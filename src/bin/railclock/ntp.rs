use core::net::{IpAddr, SocketAddr};

use alloc::format;
use defmt::{error, info};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_time::{Duration, Instant, Timer};
use mcp794xx::{NaiveDate, NaiveDateTime};
use smoltcp::wire::DnsQueryType;
use sntpc::{get_time, NtpContext, NtpTimestampGenerator};

use crate::config::NTP_SERVER;
use crate::rtc::RTC;
use crate::wifi::WifiResources;

#[derive(Copy, Clone)]
struct Timestamp {
    instant: Instant,
}

impl Default for Timestamp {
    fn default() -> Self {
        Timestamp {
            instant: Instant::now(),
        }
    }
}

impl NtpTimestampGenerator for Timestamp {
    fn init(&mut self) {
        self.instant = Instant::now();
    }

    fn timestamp_sec(&self) -> u64 {
        self.instant.as_secs()
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        (self.instant.as_millis() - self.instant.as_secs() * 1000) as u32
    }
}

#[embassy_executor::task]
pub(crate) async fn sync_time_with_ntp(stack: &'static WifiResources) {
    let WifiResources { sta_stack } = stack;
    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 4096];

    // Create UDP socket bound to ephemeral port
    let mut socket = UdpSocket::new(
        *sta_stack,
        &mut rx_meta,
        &mut rx_buf,
        &mut tx_meta,
        &mut tx_buf,
    );

    socket.bind(0).unwrap();

    let ctx = NtpContext::new(Timestamp::default());

    let ntp_addrs = sta_stack.dns_query(NTP_SERVER, DnsQueryType::A).await;
    let ntp_addrs = ntp_addrs.unwrap();

    if ntp_addrs.is_empty() {
        error!("Failed to resolve DNS");
        return;
    }

    loop {
        let addr: IpAddr = ntp_addrs[0].into();
        let result = get_time(SocketAddr::from((addr, 123)), &socket, ctx).await;

        match result {
            Ok(time) => {
                let datetime = NaiveDateTime::from_timestamp(
                    time.sec() as i64,
                    time.sec_fraction() * (1_000_000_000 / u32::MAX),
                );
                if let Err(e) = RTC.set_datetime(datetime).await {
                    error!(
                        "Failed to set RTC time, reason: {}",
                        format!("{:?}", e).as_str()
                    );
                }

                info!("Time: {:?}", time);
            }
            Err(e) => {
                error!("Error getting time: {:?}", e);
            }
        }

        Timer::after(Duration::from_secs(1200)).await;
    }
}
