use msgs::dealer::{BankStateRequest, CreateInvoiceRequest, Dealer};
use msgs::Message;
use structopt::StructOpt;
use uuid::Uuid;

#[derive(Debug, StructOpt)]
pub enum Action {
    CreateInsuranceInvoice {
        #[structopt(short = "a", long = "amount")]
        amount: u64,
    },
    GetBankState,
}

impl Action {
    pub fn into_request(self) -> Message {
        match self {
            Self::CreateInsuranceInvoice { amount } => {
                Message::Dealer(Dealer::CreateInsuranceInvoiceRequest(CreateInvoiceRequest {
                    req_id: Uuid::new_v4(),
                    amount,
                    memo: "Insurance fund top-up".to_string(),
                }))
            }
            Self::GetBankState => {
                Message::Dealer(Dealer::BankStateRequest(BankStateRequest { req_id: Uuid::new_v4() }))
            }
        }
    }
}
