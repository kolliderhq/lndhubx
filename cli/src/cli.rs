use crate::actions::Action;
use msgs::blockchain::Blockchain;
use msgs::{cli::Cli as CliMsg, dealer::Dealer, Message};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use utils::xzmq::ZmqSocket;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CliSettings {
    pub bank_cli_resp_address: String,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "lndhubx")]
pub struct Cli {
    #[structopt(subcommand)]
    action: Action,
}

impl Cli {
    pub fn execute(self, socket: ZmqSocket) -> ResponseHandler {
        let msg = self.action.into_request();
        utils::xzmq::send_as_bincode(&socket, &msg);

        ResponseHandler { socket }
    }
}

pub struct ResponseHandler {
    socket: ZmqSocket,
}

impl ResponseHandler {
    pub fn process_response(self) {
        match self.socket.recv_msg(0) {
            Ok(frame) => match serde_json::from_slice::<Message>(&frame) {
                Ok(msg) => match msg {
                    Message::Dealer(Dealer::CreateInvoiceResponse(create_invoice_response)) => {
                        println!("Received create invoice response: {:?}", create_invoice_response);
                    }
                    Message::Dealer(Dealer::BankState(state)) => {
                        println!("Received bank state: {:?}", state);
                    }
                    Message::Cli(CliMsg::MakeTxResult(tx_result)) => {
                        println!("Received transaction result: {:?}", tx_result);
                    }
                    Message::Blockchain(Blockchain::BtcReceiveAddress(address)) => {
                        println!("Received on-chain address: {:?}", address);
                    }
                    _ => {
                        println!("Received unhandled message: {:?}", msg)
                    }
                },
                Err(err) => {
                    eprintln!("Error while deserializing a payload into message: {:?}", err)
                }
            },
            Err(err) => {
                eprintln!("Error while receiving a message: {:?}", err)
            }
        }
    }
}
