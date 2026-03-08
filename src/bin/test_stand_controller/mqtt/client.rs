use defmt::{debug, error, info, warn};
use embassy_futures::select::{select, Either};
use embassy_net::tcp::TcpSocket;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};
use rust_mqtt::buffer::BumpBuffer;
use rust_mqtt::client::event::Event;
use rust_mqtt::client::options::{
    ConnectOptions, PublicationOptions, RetainHandling, SubscriptionOptions,
};
use rust_mqtt::client::Client;
use rust_mqtt::config::{KeepAlive, SessionExpiryInterval};
use rust_mqtt::types::{MqttBinary, MqttString, QoS};
use smoltcp::wire::{DnsQueryType, IpAddress};
use static_cell::StaticCell;

use crate::config::{MQTT_CLIENT_ID, MQTT_HOST, MQTT_PASSWORD, MQTT_PORT, MQTT_USER};
use crate::mqtt::codec::EncodeError;
use crate::mqtt::commands::servo::ServoCommand;
use crate::mqtt::commands::shutdown::ShutdownCommand;
use crate::mqtt::commands::state::StateCommand;
use crate::mqtt::commands::{
    CommandDispatcher, ServoCommandHandler, ShutdownCommandHandler, StateCommandHandler,
};
use crate::mqtt::queue::{self, OutboundMessage};
use crate::mqtt::sensors::status::StateStatus;
use crate::mqtt::sensors::EncodablePayload;
use crate::mqtt::topics::{
    self, TopicBuildError, COMMAND_TOPICS, TEMP_TOPIC_BUFFER_LEN, TOPIC_STATUS_CMD,
    TOPIC_STATUS_SERVO, TOPIC_STATUS_STATE,
};
use mainboard::wifi::WifiResourceSta;

const RECONNECT_DELAY_MS: u64 = 5000;
const MQTT_KEEPALIVE_SECS: u16 = 10;
const MQTT_BUFFER_SIZE: usize = 4096;
const TCP_BUFFER_SIZE: usize = 4096;
const MQTT_PAYLOAD_BUFFER_SIZE: usize = 256;

static TCP_RX_BUF: StaticCell<[u8; TCP_BUFFER_SIZE]> = StaticCell::new();
static TCP_TX_BUF: StaticCell<[u8; TCP_BUFFER_SIZE]> = StaticCell::new();
static MQTT_BUF: StaticCell<[u8; MQTT_BUFFER_SIZE]> = StaticCell::new();

type AppClient<'a, 'b> = Client<'a, TcpSocket<'b>, BumpBuffer<'a>, 5, 2, 2>;

#[derive(Debug, Clone, Copy, defmt::Format)]
enum AppMqttError {
    DnsLookupFailed,
    TcpConnectFailed,
    StringConversionError,
    EncodeError,
    MqttError,
}

struct AppCommandHandlers {
    shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>,
}

impl AppCommandHandlers {
    fn new(shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>) -> Self {
        Self { shutdown_signal }
    }
}

impl StateCommandHandler for AppCommandHandlers {
    fn handle_state_command(&mut self, command: StateCommand) {
        crate::sequencer::send_state_command(command);
        info!("MQTT command: state -> {:?}", command);
    }
}

impl ServoCommandHandler for AppCommandHandlers {
    fn handle_servo_command(&mut self, command: ServoCommand) {
        if crate::sequencer::load_state() == StateStatus::Fire {
            warn!("MQTT command ignored: cmd/servo in FIRE state");
            queue::publish_command_log("Servo command rejected: FIRE state");
            return;
        }

        match command {
            ServoCommand::Open => info!("MQTT command: OPEN"),
            ServoCommand::Close => info!("MQTT command: CLOSE"),
        }
        crate::servo::send_servo_command(command);
    }
}

impl ShutdownCommandHandler for AppCommandHandlers {
    fn handle_shutdown_command(&mut self, command: ShutdownCommand) {
        match command {
            ShutdownCommand::Shutdown => {
                if crate::sequencer::load_state() == StateStatus::Fire {
                    warn!("MQTT command ignored: SHUTDOWN in FIRE state");
                    queue::publish_command_log("Shutdown rejected: FIRE state");
                    return;
                }
                info!("MQTT command: SHUTDOWN");
                queue::publish_command_log("Shutdown");
                self.shutdown_signal.signal(());
            }
        }
    }
}

#[embassy_executor::task]
pub async fn mqtt_task(
    wifi: &'static WifiResourceSta,
    shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>,
) {
    let tcp_rx_buf = TCP_RX_BUF.init([0u8; TCP_BUFFER_SIZE]);
    let tcp_tx_buf = TCP_TX_BUF.init([0u8; TCP_BUFFER_SIZE]);
    let mqtt_buf = MQTT_BUF.init([0u8; MQTT_BUFFER_SIZE]);

    loop {
        wifi.wait_link_up().await;
        wifi.wait_config_up().await;

        if let Err(error) =
            mqtt_connection_loop(wifi, tcp_rx_buf, tcp_tx_buf, mqtt_buf, shutdown_signal).await
        {
            error!("MQTT session ended: {:?}", &error);
        }

        queue::clear_outbound_queue();
        Timer::after(Duration::from_millis(RECONNECT_DELAY_MS)).await;
    }
}

async fn mqtt_connection_loop(
    sta_stack: &embassy_net::Stack<'static>,
    tcp_rx_buf: &mut [u8; TCP_BUFFER_SIZE],
    tcp_tx_buf: &mut [u8; TCP_BUFFER_SIZE],
    mqtt_buf: &mut [u8; MQTT_BUFFER_SIZE],
    shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>,
) -> Result<(), AppMqttError> {
    let endpoint = resolve_mqtt_endpoint(sta_stack).await?;

    let mut socket = TcpSocket::new(*sta_stack, tcp_rx_buf, tcp_tx_buf);
    socket
        .connect(endpoint)
        .await
        .map_err(|_| AppMqttError::TcpConnectFailed)?;

    mqtt_buf.fill(0);
    let mut buffer = BumpBuffer::new(mqtt_buf);
    let mut client = Client::<_, _, 5, 2, 2>::new(&mut buffer);

    let connect_options = build_connect_options()?;
    let client_id =
        MqttString::from_slice(MQTT_CLIENT_ID).map_err(|_| AppMqttError::StringConversionError)?;

    client
        .connect(socket, &connect_options, Some(client_id))
        .await
        .map_err(|_| AppMqttError::MqttError)?;

    subscribe_to_commands(&mut client).await?;
    publish_state_on_connect();
    run_session_loop(&mut client, shutdown_signal).await
}

fn publish_state_on_connect() {
    crate::sequencer::republish_sequencer_state();
    crate::servo::republish_servo_state();
    crate::sequencer::republish_armed_state();
    queue::publish_command_log("Connected");
    info!("Published current state on connect");
}

async fn resolve_mqtt_endpoint(
    sta_stack: &embassy_net::Stack<'static>,
) -> Result<(smoltcp::wire::Ipv4Address, u16), AppMqttError> {
    info!("MQTT resolving host: {}", MQTT_HOST);
    let addrs = sta_stack
        .dns_query(MQTT_HOST, DnsQueryType::A)
        .await
        .map_err(|_| AppMqttError::DnsLookupFailed)?;

    let first = addrs.first().ok_or(AppMqttError::DnsLookupFailed)?;
    match first {
        IpAddress::Ipv4(ip) => Ok((*ip, MQTT_PORT)),
    }
}

fn build_connect_options() -> Result<ConnectOptions<'static>, AppMqttError> {
    let (user_name, password) = if let (Some(user), Some(pass)) = (MQTT_USER, MQTT_PASSWORD) {
        let user_name =
            MqttString::from_slice(user).map_err(|_| AppMqttError::StringConversionError)?;
        let password = MqttBinary::try_from(pass.as_bytes())
            .map_err(|_| AppMqttError::StringConversionError)?;
        (Some(user_name), Some(password))
    } else {
        (None, None)
    };

    Ok(ConnectOptions {
        clean_start: true,
        keep_alive: KeepAlive::Seconds(MQTT_KEEPALIVE_SECS),
        session_expiry_interval: SessionExpiryInterval::default(),
        user_name,
        password,
        will: None,
    })
}

async fn subscribe_to_commands(client: &mut AppClient<'_, '_>) -> Result<(), AppMqttError> {
    let options = SubscriptionOptions {
        retain_handling: RetainHandling::default(),
        retain_as_published: false,
        no_local: false,
        qos: QoS::AtLeastOnce,
    };

    for topic in COMMAND_TOPICS {
        let filter = topics::make_topic_filter(topic).ok_or(AppMqttError::StringConversionError)?;
        client
            .subscribe(filter, options)
            .await
            .map_err(|_| AppMqttError::MqttError)?;
    }

    let mut subacks = 0usize;
    while subacks < COMMAND_TOPICS.len() {
        match client.poll().await {
            Ok(Event::Suback(_)) => subacks += 1,
            Ok(event) => debug!("MQTT event during subscribe: {:?}", &event),
            Err(_) => return Err(AppMqttError::MqttError),
        }
    }

    Ok(())
}

async fn run_session_loop(
    client: &mut AppClient<'_, '_>,
    shutdown_signal: &'static Signal<CriticalSectionRawMutex, ()>,
) -> Result<(), AppMqttError> {
    let mut dispatcher = CommandDispatcher::new(AppCommandHandlers::new(shutdown_signal));
    let mut payload_buffer = [0u8; MQTT_PAYLOAD_BUFFER_SIZE];
    let mut temp_topic_buffer = [0u8; TEMP_TOPIC_BUFFER_LEN];

    loop {
        match select(client.poll_header(), queue::receive_outbound_message()).await {
            Either::First(header_result) => {
                let header = header_result.map_err(|_| AppMqttError::MqttError)?;
                let event = client
                    .poll_body(header)
                    .await
                    .map_err(|_| AppMqttError::MqttError)?;
                handle_incoming_event(event, &mut dispatcher);
            }
            Either::Second(message) => {
                publish_outbound_message(
                    client,
                    message,
                    &mut payload_buffer,
                    &mut temp_topic_buffer,
                )
                .await?;
            }
        }
    }
}

fn handle_incoming_event<H>(event: Event<'_>, dispatcher: &mut CommandDispatcher<H>)
where
    H: StateCommandHandler + ServoCommandHandler + ShutdownCommandHandler,
{
    if let Event::Publish(publish) = event {
        let topic: &str = publish.topic.as_ref();
        let payload: &[u8] = publish.message.as_ref();

        if let Err(error) = dispatcher.dispatch(topic, payload) {
            warn!("MQTT command rejected topic='{}': {:?}", topic, &error);
        }
    }
}

async fn publish_outbound_message(
    client: &mut AppClient<'_, '_>,
    message: OutboundMessage,
    payload_buffer: &mut [u8; MQTT_PAYLOAD_BUFFER_SIZE],
    temp_topic_buffer: &mut [u8; TEMP_TOPIC_BUFFER_LEN],
) -> Result<(), AppMqttError> {
    let encoded =
        encode_outbound_message(&message, payload_buffer, temp_topic_buffer).map_err(map_encode)?;

    let retain = matches!(
        message,
        OutboundMessage::Armed(_)
            | OutboundMessage::ServoSensor(_)
            | OutboundMessage::ServoStatus(_)
            | OutboundMessage::StateStatus(_)
    );

    let topic =
        topics::make_topic_name(encoded.topic).ok_or(AppMqttError::StringConversionError)?;
    let options = PublicationOptions {
        retain,
        topic,
        qos: QoS::AtMostOnce,
    };

    client
        .publish(&options, encoded.payload.into())
        .await
        .map_err(|_| AppMqttError::MqttError)?;

    Ok(())
}

struct EncodedMessage<'a> {
    topic: &'a str,
    payload: &'a [u8],
}

fn encode_outbound_message<'a>(
    message: &'a OutboundMessage,
    payload_buffer: &'a mut [u8; MQTT_PAYLOAD_BUFFER_SIZE],
    temp_topic_buffer: &'a mut [u8; TEMP_TOPIC_BUFFER_LEN],
) -> Result<EncodedMessage<'a>, EncodeErrorWithTopic> {
    let encoded = match message {
        OutboundMessage::FastAdc(packet) => {
            let written = packet
                .encode_payload(payload_buffer)
                .map_err(EncodeErrorWithTopic::Codec)?;
            EncodedMessage {
                topic: packet.topic(),
                payload: &payload_buffer[..written],
            }
        }
        OutboundMessage::SlowAdc(packet) => {
            let written = packet
                .encode_payload(payload_buffer)
                .map_err(EncodeErrorWithTopic::Codec)?;
            EncodedMessage {
                topic: packet.topic(),
                payload: &payload_buffer[..written],
            }
        }
        OutboundMessage::Armed(packet) => {
            let written = packet
                .encode_payload(payload_buffer)
                .map_err(EncodeErrorWithTopic::Codec)?;
            EncodedMessage {
                topic: packet.topic(),
                payload: &payload_buffer[..written],
            }
        }
        OutboundMessage::Temp(packet) => {
            let topic = topics::format_temp_topic(packet.sensor_id(), temp_topic_buffer)
                .map_err(EncodeErrorWithTopic::Topic)?;
            let written = packet
                .encode_payload(payload_buffer)
                .map_err(EncodeErrorWithTopic::Codec)?;
            EncodedMessage {
                topic,
                payload: &payload_buffer[..written],
            }
        }
        OutboundMessage::ServoSensor(packet) => {
            let written = packet
                .encode_payload(payload_buffer)
                .map_err(EncodeErrorWithTopic::Codec)?;
            EncodedMessage {
                topic: packet.topic(),
                payload: &payload_buffer[..written],
            }
        }
        OutboundMessage::StateStatus(status) => EncodedMessage {
            topic: TOPIC_STATUS_STATE,
            payload: status.as_bytes(),
        },
        OutboundMessage::ServoStatus(status) => EncodedMessage {
            topic: TOPIC_STATUS_SERVO,
            payload: status.as_bytes(),
        },
        OutboundMessage::CommandStatus(status) => EncodedMessage {
            topic: TOPIC_STATUS_CMD,
            payload: status.as_bytes(),
        },
    };

    Ok(encoded)
}

#[derive(Debug, Clone, Copy, defmt::Format)]
enum EncodeErrorWithTopic {
    Codec(EncodeError),
    Topic(TopicBuildError),
}

fn map_encode(_error: EncodeErrorWithTopic) -> AppMqttError {
    AppMqttError::EncodeError
}
