pub use zmq::{Context as ZmqContext, Socket as ZmqSocket, SocketType};

pub fn create_publisher(address: &str) -> ZmqSocket {
    let context = ZmqContext::new();
    let socket = context.socket(zmq::PUB).unwrap();
    socket.bind(address).unwrap();
    socket
}

pub fn create_subscriber(address: &str) -> ZmqSocket {
    let context = ZmqContext::new();
    let socket = context.socket(zmq::SUB).unwrap();
    socket.set_subscribe(&[]).unwrap();
    socket.connect(address).unwrap();
    socket
}

pub fn create_push(address: &str) -> ZmqSocket {
    let context = ZmqContext::new();
    let socket = context.socket(zmq::PUSH).unwrap();
    socket.connect(address).unwrap();
    socket
}

pub fn create_pull(address: &str) -> ZmqSocket {
    let context = ZmqContext::new();
    let socket = context.socket(zmq::PULL).unwrap();
    socket.bind(address).unwrap();
    socket
}
