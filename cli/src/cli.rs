use msgs::Message;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CliSettings {
    pub bank_cli_resp_address: String,
}

pub struct Cli {}

impl Cli {
    pub fn new() -> Self {
        Self {}
    }
    pub fn process_msg(&mut self, msg: Message) {}
}

impl Default for Cli {
    fn default() -> Self {
        Self::new()
    }
}
