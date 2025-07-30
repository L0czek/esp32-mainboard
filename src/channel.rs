use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

pub struct RequestResponseChannel<Req, Resp, const N: usize> {
    req_channel: Channel<CriticalSectionRawMutex, Req, N>,
    resp_channel: Channel<CriticalSectionRawMutex, Resp, N>,
}

impl<Req, Resp, const N: usize> RequestResponseChannel<Req, Resp, N> {
    pub const fn with_static_channels() -> Self {
        Self {
            req_channel: Channel::new(),
            resp_channel: Channel::new(),
        }
    }

    pub async fn send_request(&self, request: Req) {
        self.req_channel.send(request).await;
    }

    pub async fn recv_request(&self) -> Req {
        self.req_channel.receive().await
    }

    pub async fn send_response(&self, response: Resp) {
        self.resp_channel.send(response).await;
    }

    pub async fn recv_response(&self) -> Resp {
        self.resp_channel.receive().await
    }

    pub async fn transact(&self, request: Req) -> Resp {
        self.send_request(request).await;
        self.recv_response().await
    }
}
