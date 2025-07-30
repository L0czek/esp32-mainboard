use embassy_sync::channel::Channel;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use once_cell::sync::OnceCell;

pub struct RequestResponseChannel<Req, Resp, const N: usize> {
    req_channel: OnceCell<Channel<CriticalSectionRawMutex, Req, N>>,
    resp_channel: OnceCell<Channel<CriticalSectionRawMutex, Resp, N>>,
}

impl<Req, Resp, const N: usize> RequestResponseChannel<Req, Resp, N> {
    pub const fn with_static_channels() -> Self {
        Self {
            req_channel: OnceCell::with_value(Channel::new()),
            resp_channel: OnceCell::with_value(Channel::new()),
        }
    }

    pub async fn send_request(&self, request: Req) {
        self.req_channel.get().unwrap().send(request).await;
    }

    pub async fn recv_request(&self) -> Req {
        self.req_channel.get().unwrap().receive().await
    }

    pub async fn send_response(&self, response: Resp) {
        self.resp_channel.get().unwrap().send(response).await;
    }

    pub async fn recv_response(&self) -> Resp {
        self.resp_channel.get().unwrap().receive().await
    }

    pub async fn transact(&self, request: Req) -> Resp {
        self.send_request(request).await;
        self.recv_response().await
    }
}

