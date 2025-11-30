use defmt::{error, info};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::pubsub::PubSubChannel;
use esp_hal::uart::{UartRx, UartTx};
use esp_hal::Async;

extern crate alloc;
use alloc::vec::Vec;

/// Maximum size for a single UART receive batch
const MAX_UART_BATCH: usize = 256;

/// UART receive data that will be published to subscribers
#[derive(Debug, Clone)]
pub struct UartReceiveData {
    pub bytes: Vec<u8>,
}

/// Global pubsub channel for UART received data
/// Capacity: 4 messages, 4 subscribers, 1 publisher
pub static UART_RX_DATA: PubSubChannel<CriticalSectionRawMutex, UartReceiveData, 4, 4, 1> = 
    PubSubChannel::new();

/// UART TX command channel - for sending data from WebSocket to UART
pub static UART_TX_CHANNEL: Channel<CriticalSectionRawMutex, Vec<u8>, 4> = Channel::new();

/// Send data via UART (queues it for transmission)
pub async fn uart_send(data: &[u8]) {
    UART_TX_CHANNEL.send(data.to_vec()).await;
}

/// Task to handle UART reception
/// Continuously reads from UART and publishes received data
#[embassy_executor::task]
pub async fn uart_receive_task(mut uart_rx: UartRx<'static, Async>) {
    info!("UART receive task started");
    
    let publisher = UART_RX_DATA.publisher().unwrap();
    let mut buffer = [0u8; MAX_UART_BATCH];
    
    loop {
        // Use read_async for async UART reading
        match uart_rx.read_async(&mut buffer).await {
            Ok(n) => {
                if n > 0 {
                    let data = UartReceiveData {
                        bytes: buffer[..n].to_vec(),
                    };
                    
                    // Publish to all subscribers
                    publisher.publish(data).await;
                    info!("UART received {} bytes", n);
                }
            }
            Err(_) => {
                error!("UART read error");
            }
        }
    }
}

/// Task to handle UART transmission
/// Waits for data from the TX channel and sends it via UART
#[embassy_executor::task]
pub async fn uart_transmit_task(mut uart_tx: UartTx<'static, Async>) {
    info!("UART transmit task started");
    
    loop {
        let data = UART_TX_CHANNEL.receive().await;
        
        match uart_tx.write_async(&data).await {
            Ok(_) => {
                info!("UART sent {} bytes", data.len());
            }
            Err(_) => {
                error!("UART write error");
            }
        }
    }
}
