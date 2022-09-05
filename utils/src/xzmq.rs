pub use zmq::{Context as ZmqContext, Socket as ZmqSocket, SocketType};

#[derive(Clone)]
pub struct SocketContext {
    context: ZmqContext,
}

impl SocketContext {
    pub fn new() -> Self {
        Self {
            context: ZmqContext::new(),
        }
    }

    pub fn create_publisher(&self, address: &str) -> ZmqSocket {
        let socket = self.context.socket(zmq::PUB).unwrap();
        socket.bind(address).unwrap();
        socket
    }

    pub fn create_subscriber(&self, address: &str) -> ZmqSocket {
        let socket = self.context.socket(zmq::SUB).unwrap();
        socket.set_subscribe(&[]).unwrap();
        socket.connect(address).unwrap();
        socket
    }

    pub fn create_push(&self, address: &str) -> ZmqSocket {
        let socket = self.context.socket(zmq::PUSH).unwrap();
        socket.connect(address).unwrap();
        socket
    }

    pub fn create_pull(&self, address: &str) -> ZmqSocket {
        let socket = self.context.socket(zmq::PULL).unwrap();
        socket.bind(address).unwrap();
        socket
    }

    pub fn create_request(&self, address: &str) -> ZmqSocket {
        let socket = self.context.socket(zmq::REQ).unwrap();
        socket.connect(address).unwrap();
        socket
    }

    pub fn create_response(&self, address: &str) -> ZmqSocket {
        let socket = self.context.socket(zmq::REP).unwrap();
        socket.bind(address).unwrap();
        socket
    }
}

impl Default for SocketContext {
    fn default() -> Self {
        Self::new()
    }
}
