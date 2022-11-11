use crate::actions::Action;
use msgs::{cli::Cli as CliMsg, dealer::Dealer, Message};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use utils::kafka::{Consumer, Producer};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CliSettings {
    pub kafka_broker_addresses: String,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "lndhubx")]
pub struct Cli {
    #[structopt(subcommand)]
    action: Action,
}

impl Cli {
    pub fn execute(self, producer: Producer, consumer: Consumer) -> ResponseHandler {
        let msg = self.action.into_request();
        producer.produce("bank", &msg);

        ResponseHandler { consumer }
    }
}

pub struct ResponseHandler {
    consumer: Consumer,
}

impl ResponseHandler {
    pub fn process_response(mut self) {
        match self.consumer.consume::<Message>() {
            Some(payload) => match payload {
                Some(msg) => match msg {
                    Message::Dealer(Dealer::CreateInvoiceResponse(create_invoice_response)) => {
                        println!("Received create invoice response: {:?}", create_invoice_response);
                    }
                    Message::Dealer(Dealer::BankState(state)) => {
                        println!("Received bank state: {:?}", state);
                    }
                    Message::Cli(CliMsg::MakeTxResult(tx_result)) => {
                        println!("Received transaction result: {:?}", tx_result);
                    }
                    _ => {
                        println!("Received unhandled message: {:?}", msg)
                    }
                },
                None => {
                    eprintln!("Received unsupported message format")
                }
            },
            None => {
                eprintln!("Failed to receive a message")
            }
        }
    }
}
