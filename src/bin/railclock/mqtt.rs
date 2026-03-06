use crate::{
    config::{
        MQTT_BATTERY_SENSOR_CONFIG_TOPIC, MQTT_BATTERY_SENSOR_DISCOVERY, MQTT_BATTERY_SENSOR_TOPIC,
        MQTT_BUTTON_CONFIG_TOPIC, MQTT_PUSH_BUTTON_DISCOVERY, MQTT_BUTTON_TOPIC,
        MQTT_NTP_SYNC_CONFIG_TOPIC, MQTT_NTP_SYNC_DISCOVERY, MQTT_NTP_SYNC_TOPIC,
            MQTT_SHUTDOWN_CONFIG_TOPIC, MQTT_SHUTDOWN_TOPIC, MQTT_SHUTDOWN_DISCOVERY,
    },
    mqtt_queue::{OutgoingMessage, OUTGOING_CH},
};
use alloc::format;
use defmt::{error, info, warn};
use embassy_futures::select::{select, Either};
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use rust_mqtt::{
    buffer::BumpBuffer,
    client::{
        event::Event,
        options::{ConnectOptions, PublicationOptions, RetainHandling, SubscriptionOptions},
        Client,
    },
    config::{KeepAlive, SessionExpiryInterval},
    types::{MqttBinary, MqttString, QoS, TopicFilter, TopicName},
};
use smoltcp::wire::{DnsQueryType, IpAddress};
use static_cell::StaticCell;

use crate::config::{MQTT_CLIENT_ID, MQTT_HOST, MQTT_PASSWORD, MQTT_PORT, MQTT_USER};
use crate::CLOCK_DRIVER;
use mainboard::wifi::WifiResourceSta;
// battery handle removed; battery task moved into binary and publishes via mqtt_queue

const RECONNECT_DELAY_MS: u64 = 5000;
const MQTT_KEEPALIVE_SECS: u16 = 10;
const BUFFER_SIZE: usize = 4096;

// Static buffers for MQTT - allocated once, reused across reconnections
static TCP_RX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
static TCP_TX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
static MQTT_BUF: StaticCell<[u8; BUFFER_SIZE]> = StaticCell::new();

// Client type alias for convenience
type AppClient<'a, 'b> = Client<'a, TcpSocket<'b>, BumpBuffer<'a>, 5, 2, 2>;

fn make_topic_name(topic: &str) -> Option<TopicName<'_>> {
    if topic.contains('#') || topic.contains('+') {
        return None;
    }
    if topic.is_empty() {
        return None;
    }
    let mqtt_string = MqttString::from_slice(topic).ok()?;
    Some(unsafe { TopicName::new_unchecked(mqtt_string) })
}

fn make_topic_filter(topic: &str) -> Option<TopicFilter<'_>> {
    if topic.is_empty() {
        return None;
    }
    let mqtt_string = MqttString::from_slice(topic).ok()?;
    Some(unsafe { TopicFilter::new_unchecked(mqtt_string) })
}

async fn subscribe_to_topic(
    client: &mut AppClient<'_, '_>,
    topic: &str,
    options: SubscriptionOptions,
) -> Result<(), AppMqttError> {
    let filter = make_topic_filter(topic).ok_or(AppMqttError::StringConversionError)?;
    info!("MQTT: Subscribing to topic: {}", topic);
    client
        .subscribe(filter, options)
        .await
        .map_err(|_| AppMqttError::MqttError)?;

    loop {
        match client.poll().await {
            Ok(Event::Suback(_)) => {
                info!("MQTT: Subscribed successfully to {}", topic);
                break;
            }
            Ok(event) => {
                info!(
                    "MQTT: Received event while waiting for SUBACK: {:?}",
                    &event
                );
            }
            Err(_) => return Err(AppMqttError::MqttError),
        }
    }

    Ok(())
}

async fn publish_discovery(
    client: &mut AppClient<'_, '_>,
    config_topic: &str,
    payload: &str,
) -> Result<(), AppMqttError> {
    let cfg_topic = make_topic_name(config_topic).ok_or(AppMqttError::StringConversionError)?;
    info!("Sending discovery payload = {}", payload);
    let cfg_options = PublicationOptions {
        retain: true,
        topic: cfg_topic,
        qos: QoS::AtMostOnce,
    };

    client
        .publish(&cfg_options, payload.as_bytes().into())
        .await
        .map_err(|_| AppMqttError::MqttError)?;

    Ok(())
}

#[embassy_executor::task]
pub(crate) async fn mqtt_task(sta_stack: &'static WifiResourceSta) {
    // Initialize static buffers once
    let tcp_rx_buf = TCP_RX_BUF.init([0u8; 4096]);
    let tcp_tx_buf = TCP_TX_BUF.init([0u8; 4096]);
    let mqtt_buf = MQTT_BUF.init([0u8; BUFFER_SIZE]);

    // Wait for network to be ready
    info!("MQTT: Waiting for network link...");
    sta_stack.wait_link_up().await;
    info!("MQTT: Link is up, waiting for IP configuration...");
    sta_stack.wait_config_up().await;
    info!("MQTT: Network configured");

    loop {
        if let Err(e) = mqtt_connection_loop(sta_stack, tcp_rx_buf, tcp_tx_buf, mqtt_buf).await {
            error!("MQTT connection error: {:?}", &e);
        }
        warn!("MQTT: Reconnecting in {} ms...", RECONNECT_DELAY_MS);
        Timer::after(Duration::from_millis(RECONNECT_DELAY_MS)).await;
    }
}

#[derive(Debug, defmt::Format)]
enum AppMqttError {
    DnsLookupFailed,
    TcpConnectFailed,
    StringConversionError,
    MqttError,
}

async fn mqtt_connection_loop(
    sta_stack: &embassy_net::Stack<'static>,
    tcp_rx_buf: &mut [u8; 4096],
    tcp_tx_buf: &mut [u8; 4096],
    mqtt_buf: &mut [u8; BUFFER_SIZE],
) -> Result<(), AppMqttError> {
    info!("MQTT: Resolving host: {}", MQTT_HOST);
    let mqtt_addrs = sta_stack
        .dns_query(MQTT_HOST, DnsQueryType::A)
        .await
        .map_err(|_| AppMqttError::DnsLookupFailed)?;

    let mqtt_ip = mqtt_addrs.first().ok_or(AppMqttError::DnsLookupFailed)?;
    info!("MQTT: Resolved to {:?}", mqtt_ip);

    let remote_endpoint = match mqtt_ip {
        IpAddress::Ipv4(ip) => (*ip, MQTT_PORT),
    };

    let mut socket = TcpSocket::new(*sta_stack, tcp_rx_buf, tcp_tx_buf);

    info!("MQTT: Connecting TCP to port {}", MQTT_PORT);
    socket
        .connect(remote_endpoint)
        .await
        .map_err(|_| AppMqttError::TcpConnectFailed)?;
    info!("MQTT: TCP connected");

    mqtt_buf.fill(0);
    let mut buffer = BumpBuffer::new(mqtt_buf);

    let mut client = Client::<_, _, 5, 2, 2>::new(&mut buffer);

    let (user_name, password) = if let (Some(user), Some(pass)) = (MQTT_USER, MQTT_PASSWORD) {
        let username =
            MqttString::from_slice(user).map_err(|_| AppMqttError::StringConversionError)?;
        let password = MqttBinary::try_from(pass.as_bytes())
            .map_err(|_| AppMqttError::StringConversionError)?;
        (Some(username), Some(password))
    } else {
        (None, None)
    };

    let connect_options = ConnectOptions {
        clean_start: true,
        keep_alive: KeepAlive::Infinite,
        session_expiry_interval: SessionExpiryInterval::default(),
        user_name,
        password,
        will: None,
    };

    let client_id =
        MqttString::from_slice(MQTT_CLIENT_ID).map_err(|_| AppMqttError::StringConversionError)?;

    info!("MQTT: Connecting to broker...");
    let connect_info = client
        .connect(socket, &connect_options, Some(client_id))
        .await
        .map_err(|_| AppMqttError::MqttError)?;
    info!(
        "MQTT: Connected to broker, session present: {}",
        connect_info.session_present
    );

    // Subscribe to button topics
    let subscription_options = SubscriptionOptions {
        retain_handling: RetainHandling::default(),
        retain_as_published: false,
        no_local: false,
        qos: QoS::AtMostOnce,
    };

    subscribe_to_topic(
        &mut client,
        MQTT_BUTTON_TOPIC.as_str(),
        subscription_options,
    )
    .await?;
    subscribe_to_topic(
        &mut client,
        MQTT_NTP_SYNC_TOPIC.as_str(),
        subscription_options,
    )
    .await?;
    subscribe_to_topic(
        &mut client,
        MQTT_SHUTDOWN_TOPIC.as_str(),
        subscription_options,
    )
    .await?;

    publish_discovery(
        &mut client,
        &MQTT_BATTERY_SENSOR_CONFIG_TOPIC,
        MQTT_BATTERY_SENSOR_DISCOVERY.as_str(),
    )
    .await?;

    publish_discovery(
        &mut client,
        &MQTT_BUTTON_CONFIG_TOPIC,
        MQTT_PUSH_BUTTON_DISCOVERY.as_str(),
    )
    .await?;

    publish_discovery(
        &mut client,
        &MQTT_NTP_SYNC_CONFIG_TOPIC,
        MQTT_NTP_SYNC_DISCOVERY.as_str(),
    )
    .await?;

    publish_discovery(
        &mut client,
        &MQTT_SHUTDOWN_CONFIG_TOPIC,
        MQTT_SHUTDOWN_DISCOVERY.as_str(),
    )
    .await?;

    loop {
        match select(client.poll_header(), OUTGOING_CH.receive()).await {
            Either::First(poll_header_res) => match poll_header_res {
                Ok(header) => match client.poll_body(header).await {
                    Ok(Event::Publish(publish)) => {
                        let topic_str: &str = publish.topic.as_ref();
                        let payload: &[u8] = publish.message.as_ref();

                        info!(
                            "MQTT: Received message on '{}': {} bytes",
                            topic_str,
                            payload.len()
                        );

                        match topic_str {
                            x if x == MQTT_BUTTON_TOPIC.as_str()
                                && payload == "PRESS".as_bytes() =>
                            {
                                info!("Received press via mqtt");
                                let driver: &crate::driver::ClockDriver = CLOCK_DRIVER.get().await;
                                driver.push_forward(1);
                            }
                            x if x == MQTT_NTP_SYNC_TOPIC.as_str()
                                && payload == "PRESS".as_bytes() =>
                            {
                                info!("Received ntp sync via mqtt");
                                crate::NTP_TRIGGER.signal(());
                            }
                            x if x == MQTT_SHUTDOWN_TOPIC.as_str()
                                && payload == "PRESS".as_bytes() =>
                            {
                                info!("Received shutdown via mqtt");
                                crate::SHUTDOWN_SIGNAL.signal(());
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::PublishAcknowledged(_)) => {}
                    Ok(Event::PublishComplete(_)) => {}
                    Ok(event) => {
                        info!("MQTT: Received event: {:?}", &event);
                    }
                    Err(e) => {
                        error!("MQTT: Poll body error: {:?}", &e);
                        return Err(AppMqttError::MqttError);
                    }
                },
                Err(e) => {
                    error!("MQTT: Poll header error: {:?}", &e);
                    return Err(AppMqttError::MqttError);
                }
            },
            Either::Second(msg) => {
                // Outgoing message received
                match msg {
                    OutgoingMessage::Publish {
                        topic,
                        payload,
                        retain,
                    } => {
                        if let Some(pub_topic) = make_topic_name(topic) {
                            let pub_opts = PublicationOptions {
                                retain,
                                topic: pub_topic,
                                qos: QoS::AtMostOnce,
                            };
                            let _ = client.publish(&pub_opts, payload.as_bytes().into()).await;
                        }
                    }
                }
            }
        }
    }
}
