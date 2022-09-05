pub mod cli;

use crate::cli::Cli;
use msgs::Message;
use utils::xzmq::ZmqSocket;

pub fn start(socket: &ZmqSocket) {
    let mut cli = Cli::new();
    while let Ok(frame) = socket.recv_msg(0) {
        if let Ok(message) = bincode::deserialize::<Message>(&frame) {
            cli.process_msg(message);
        };
    }
}
