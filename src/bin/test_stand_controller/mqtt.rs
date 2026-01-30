use defmt::{error, info, warn};
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
use crate::wifi::WifiResources;

const RECONNECT_DELAY_MS: u64 = 5000;
const KEEPALIVE_SECS: u16 = 60;
const BUFFER_SIZE: usize = 4096;

// Static buffers for MQTT - allocated once, reused across reconnections
static TCP_RX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
static TCP_TX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
static MQTT_BUF: StaticCell<[u8; BUFFER_SIZE]> = StaticCell::new();

/// Creates a TopicName from a string slice, validating that it's a valid MQTT topic name.
/// Returns None if the topic contains wildcard characters (# or +) or is invalid.
fn make_topic_name(topic: &str) -> Option<TopicName<'_>> {
    // MQTT topic names cannot contain wildcard characters
    if topic.contains('#') || topic.contains('+') {
        return None;
    }
    // Topic name cannot be empty
    if topic.is_empty() {
        return None;
    }
    let mqtt_string = MqttString::from_slice(topic).ok()?;
    // SAFETY: We've validated that the string contains no wildcard characters
    // and is a valid UTF-8 string within MQTT length limits
    Some(unsafe { TopicName::new_unchecked(mqtt_string) })
}

/// Creates a TopicFilter from a string slice. TopicFilters can contain wildcards.
fn make_topic_filter(topic: &str) -> Option<TopicFilter<'_>> {
    if topic.is_empty() {
        return None;
    }
    let mqtt_string = MqttString::from_slice(topic).ok()?;
    // SAFETY: We've validated that the string is a valid UTF-8 string within MQTT length limits.
    // Wildcard validation for filters is more complex, but we trust the caller for now.
    Some(unsafe { TopicFilter::new_unchecked(mqtt_string) })
}

#[embassy_executor::task]
pub(crate) async fn mqtt_task(wifi: &'static WifiResources) {
    let WifiResources { sta_stack } = wifi;

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
    // Resolve MQTT host
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

    // Create TCP socket with static buffers
    let mut socket = TcpSocket::new(*sta_stack, tcp_rx_buf, tcp_tx_buf);
    socket.set_timeout(Some(Duration::from_secs(30)));
    socket.set_keep_alive(Some(Duration::from_secs(KEEPALIVE_SECS as u64)));

    // Connect TCP socket
    info!("MQTT: Connecting TCP to port {}", MQTT_PORT);
    socket
        .connect(remote_endpoint)
        .await
        .map_err(|_| AppMqttError::TcpConnectFailed)?;
    info!("MQTT: TCP connected");

    // Create MQTT buffer (reset for reuse)
    mqtt_buf.fill(0);
    let mut buffer = BumpBuffer::new(mqtt_buf);

    // Create MQTT client
    let mut client = Client::<_, _, 5, 2, 2>::new(&mut buffer);

    // Build connect options with credentials if provided
    let (user_name, password) = if let (Some(user), Some(pass)) = (MQTT_USER, MQTT_PASSWORD) {
        let username = MqttString::from_slice(user).map_err(|_| AppMqttError::StringConversionError)?;
        let password = MqttBinary::try_from(pass.as_bytes()).map_err(|_| AppMqttError::StringConversionError)?;
        (Some(username), Some(password))
    } else {
        (None, None)
    };

    let connect_options = ConnectOptions {
        clean_start: true,
        keep_alive: KeepAlive::Seconds(KEEPALIVE_SECS),
        session_expiry_interval: SessionExpiryInterval::default(),
        user_name,
        password,
        will: None,
    };

    // Create client identifier
    let client_id =
        MqttString::from_slice(MQTT_CLIENT_ID).map_err(|_| AppMqttError::StringConversionError)?;

    // Connect to MQTT broker
    info!("MQTT: Connecting to broker...");
    let connect_info = client
        .connect(socket, &connect_options, Some(client_id))
        .await
        .map_err(|_| AppMqttError::MqttError)?;
    info!(
        "MQTT: Connected to broker, session present: {}",
        connect_info.session_present
    );

    // Subscribe to command topic
    let subscribe_topic =
        make_topic_filter("test-stand/command").ok_or(AppMqttError::StringConversionError)?;
    info!("MQTT: Subscribing to topic: test-stand/command");

    let subscription_options = SubscriptionOptions {
        retain_handling: RetainHandling::default(),
        retain_as_published: false,
        no_local: false,
        qos: QoS::AtLeastOnce,
    };

    let _sub_id = client
        .subscribe(subscribe_topic, subscription_options)
        .await
        .map_err(|_| AppMqttError::MqttError)?;

    // Wait for SUBACK
    loop {
        match client.poll().await {
            Ok(Event::Suback(_)) => {
                info!("MQTT: Subscribed successfully");
                break;
            }
            Ok(event) => {
                info!("MQTT: Received event while waiting for SUBACK: {:?}",&event);
            }
            Err(e) => {
                error!("MQTT: Error waiting for SUBACK: {:?}", &e);
                return Err(AppMqttError::MqttError);
            }
        }
    }

    // Publish online status
    let status_topic =
        make_topic_name("test-stand/status").ok_or(AppMqttError::StringConversionError)?;

    let pub_options = PublicationOptions {
        retain: false,
        topic: status_topic.clone(),
        qos: QoS::AtLeastOnce,
    };

    client
        .publish(&pub_options, b"online".as_slice().into())
        .await
        .map_err(|_| AppMqttError::MqttError)?;
    info!("MQTT: Published online status");

    // Main message loop
    loop {
        match client.poll().await {
            Ok(Event::Publish(publish)) => {
                let topic_str: &str = publish.topic.as_ref();
                let payload: &[u8] = publish.message.as_ref();

                info!(
                    "MQTT: Received message on '{}': {} bytes",
                    topic_str,
                    payload.len()
                );

                // Echo back on response topic
                let response_topic =
                    make_topic_name("test-stand/response").ok_or(AppMqttError::StringConversionError)?;

                let response_options = PublicationOptions {
                    retain: false,
                    topic: response_topic,
                    qos: QoS::AtLeastOnce,
                };

                client
                    .publish(&response_options, payload.into())
                    .await
                    .map_err(|_| AppMqttError::MqttError)?;
            }
            Ok(Event::PublishAcknowledged(_)) => {
                // QoS 1 publish acknowledged
            }
            Ok(Event::PublishComplete(_)) => {
                // QoS 2 publish complete
            }
            Ok(event) => {
                info!("MQTT: Received event: {:?}",&event);
            }
            Err(e) => {
                error!("MQTT: Poll error: {:?}", &e);
                // Try to publish offline status (best effort)
                let offline_options = PublicationOptions {
                    retain: false,
                    topic: status_topic,
                    qos: QoS::AtMostOnce,
                };
                let _ = client.publish(&offline_options, b"offline".as_slice().into()).await;
                return Err(AppMqttError::MqttError);
            }
        }
    }
}
