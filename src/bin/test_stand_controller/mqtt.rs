use embassy_net::tcp::{TcpSocket, client::TcpConnection};
use rust_mqtt::{buffer::BumpBuffer, client::Client};
use smoltcp::wire::DnsQueryType;
use static_cell::StaticCell;

use crate::{config::MQTT_HOST, wifi::WifiResources};

static BUFFER: StaticCell<[u8; 0x1000]> = StaticCell::new();

#[embassy_executor::task]
pub(crate) async fn mqtt_task(stack: &'static WifiResources) {
    let buffer = BUFFER.init([0u8; 0x1000]);
    let mut allocator = BumpBuffer::new(buffer);
    let WifiResources { sta_stack } = stack;
    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 4096];

    let mqtt_addrs = sta_stack
        .dns_query(MQTT_HOST, DnsQueryType::A)
        .await
        .unwrap();

    let mqtt_addr = mqtt_addrs[0];
    let mut socket = TcpSocket::new(*sta_stack, &mut rx_buf, &mut tx_buf);
    socket.connect();

    todo!();
    // let buffer = Client::<'_, _, 1, 1, 1>::new(&mut allocator);


}
