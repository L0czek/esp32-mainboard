use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use defmt::error;
use embedded_hal_bus::i2c::AtomicError;
use mainboard::{board::acquire_i2c_bus, channel::RequestResponseChannel};
use mcp794xx::NaiveDateTime;
use mcp794xx::DateTimeAccess;
use defmt::info;
use rust_mqtt::client::info;

#[derive(Debug)]
pub(crate) enum RtcRequest {
    GetDateTime(),
    SetDateTime(NaiveDateTime),

    ReadNonvolatileMem {
        addr: u8,
        size: u8
    },

    WriteNonvolatileMem {
        addr: u8,
        data: Vec<u8>
    }
    ,
    EnableAlarm(mcp794xx::Alarm),
    DisableAlarm(mcp794xx::Alarm),
    SetAlarm {
        alarm: mcp794xx::Alarm,
        when: mcp794xx::AlarmDateTime,
        matching: mcp794xx::AlarmMatching,
        polarity: mcp794xx::AlarmOutputPinPolarity,
    }
    ,
    HasAlarmMatched(mcp794xx::Alarm),
    ClearAlarmMatchedFlag(mcp794xx::Alarm)
}

pub(crate) enum RtcResponse {
    Ok,

    RtcError(mcp794xx::Error<AtomicError<esp_hal::i2c::master::Error>>),

    NonvolatileMem(Vec<u8>),
    DateTime(NaiveDateTime)
    ,
    HasAlarmMatched(bool)
}

pub(crate) static RTC_CHANNEL: RequestResponseChannel<RtcRequest, RtcResponse, 10> = RequestResponseChannel::with_static_channels();
pub(crate) static RTC: RtcClient = RtcClient::new();

pub(crate) struct RtcNonvolatileState {
    pub last_update: NaiveDateTime
}

#[derive(Clone, Copy)]
pub(crate) struct RtcClient;

#[derive(Debug)]
pub(crate) enum RtcClientError {
    Rtc(mcp794xx::Error<AtomicError<esp_hal::i2c::master::Error>>),
    UnexpectedResponse,
}

impl From<mcp794xx::Error<AtomicError<esp_hal::i2c::master::Error>>> for RtcClientError {
    fn from(e: mcp794xx::Error<AtomicError<esp_hal::i2c::master::Error>>) -> Self {
        RtcClientError::Rtc(e)
    }
}

impl RtcClient {
    pub const fn new() -> Self {
        RtcClient
    }

    pub async fn get_datetime(&self) -> Result<NaiveDateTime, RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::GetDateTime()).await {
            RtcResponse::DateTime(v) => Ok(v),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn set_datetime(&self, dt: NaiveDateTime) -> Result<(), RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::SetDateTime(dt)).await {
            RtcResponse::Ok => Ok(()),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn read_nonvolatile(&self, addr: u8, size: u8) -> Result<Vec<u8>, RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::ReadNonvolatileMem { addr, size }).await {
            RtcResponse::NonvolatileMem(v) => Ok(v),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn write_nonvolatile(&self, addr: u8, data: &[u8]) -> Result<(), RtcClientError> {
        match RTC_CHANNEL
            .transact(RtcRequest::WriteNonvolatileMem { addr, data: data.to_vec() })
            .await
        {
            RtcResponse::Ok => Ok(()),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn enable_alarm(&self, alarm: mcp794xx::Alarm) -> Result<(), RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::EnableAlarm(alarm)).await {
            RtcResponse::Ok => Ok(()),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn disable_alarm(&self, alarm: mcp794xx::Alarm) -> Result<(), RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::DisableAlarm(alarm)).await {
            RtcResponse::Ok => Ok(()),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn set_alarm(
        &self,
        alarm: mcp794xx::Alarm,
        when: mcp794xx::AlarmDateTime,
        matching: mcp794xx::AlarmMatching,
        polarity: mcp794xx::AlarmOutputPinPolarity,
    ) -> Result<(), RtcClientError> {
        match RTC_CHANNEL
            .transact(RtcRequest::SetAlarm {
                alarm,
                when,
                matching,
                polarity,
            })
            .await
        {
            RtcResponse::Ok => Ok(()),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn has_alarm_matched(&self, alarm: mcp794xx::Alarm) -> Result<bool, RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::HasAlarmMatched(alarm)).await {
            RtcResponse::HasAlarmMatched(v) => Ok(v),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }

    pub async fn clear_alarm_matched_flag(&self, alarm: mcp794xx::Alarm) -> Result<(), RtcClientError> {
        match RTC_CHANNEL.transact(RtcRequest::ClearAlarmMatchedFlag(alarm)).await {
            RtcResponse::Ok => Ok(()),
            RtcResponse::RtcError(e) => Err(RtcClientError::Rtc(e)),
            _ => Err(RtcClientError::UnexpectedResponse),
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn rtc_handler() {
    let mut rtc = mcp794xx::Mcp794xx::new_mcp79400(acquire_i2c_bus());

    loop {
        let request = RTC_CHANNEL.recv_request().await;

        let response = match request {
            RtcRequest::SetDateTime(datetime) => {
                let ret = match rtc.set_datetime(&datetime) {
                    Ok(()) => RtcResponse::Ok,
                    Err(e) => RtcResponse::RtcError(e)
                };
                if let Err(e) = rtc.enable() {
                    error!("Failed to enable RTC {:?}", format!("{:?}", e).as_str());
                }

                ret
            },

            RtcRequest::ReadNonvolatileMem { addr, size } => {
                let mut mem = Vec::with_capacity(size as usize);
                mem.resize(size as usize, 0u8);

                info!("addr: {} size: {}", addr, mem.as_mut_slice().len());
                match rtc.read_sram_data(addr, mem.as_mut_slice()) {
                    Ok(()) => RtcResponse::NonvolatileMem(mem),
                    Err(e) => RtcResponse::RtcError(e)
                }
            }

            RtcRequest::WriteNonvolatileMem { addr, data } => {
                match rtc.write_sram_data(addr, data.as_ref()) {
                    Ok(()) => RtcResponse::Ok,
                    Err(e) => RtcResponse::RtcError(e)
                }
            },

            RtcRequest::EnableAlarm(alarm) => {
                match rtc.enable_alarm(alarm) {
                    Ok(()) => RtcResponse::Ok,
                    Err(e) => RtcResponse::RtcError(e),
                }
            },

            RtcRequest::DisableAlarm(alarm) => {
                match rtc.disable_alarm(alarm) {
                    Ok(()) => RtcResponse::Ok,
                    Err(e) => RtcResponse::RtcError(e),
                }
            },

            RtcRequest::SetAlarm { alarm, when, matching, polarity } => {
                match rtc.set_alarm(alarm, when, matching, polarity) {
                    Ok(()) => RtcResponse::Ok,
                    Err(e) => RtcResponse::RtcError(e),
                }
            },

            RtcRequest::HasAlarmMatched(alarm) => {
                match rtc.has_alarm_matched(alarm) {
                    Ok(v) => RtcResponse::HasAlarmMatched(v),
                    Err(e) => RtcResponse::RtcError(e),
                }
            },

            RtcRequest::ClearAlarmMatchedFlag(alarm) => {
                match rtc.clear_alarm_matched_flag(alarm) {
                    Ok(()) => RtcResponse::Ok,
                    Err(e) => RtcResponse::RtcError(e),
                }
            },

            RtcRequest::GetDateTime() => {
                match rtc.datetime() {
                    Ok(v) => RtcResponse::DateTime(v),
                    Err(e) => RtcResponse::RtcError(e)
                }
            }
        };

        RTC_CHANNEL.send_response(response).await;
    }
}
