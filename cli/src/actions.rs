use core_types::{Currency, UserId};
use msgs::cli::{Cli, MakeTx};
use msgs::dealer::{BankStateRequest, CreateInvoiceRequest, Dealer};
use msgs::nostr::{Nostr, NostrProfilesRefetchRequest};
use msgs::Message;
use rust_decimal::Decimal;
use structopt::StructOpt;
use uuid::Uuid;

#[derive(Debug, StructOpt)]
pub enum Action {
    CreateInsuranceInvoice {
        #[structopt(short = "a", long = "amount")]
        amount: u64,
    },
    GetBankState,
    MakeTx {
        #[structopt(long = "outbound_uid")]
        outbound_uid: UserId,
        #[structopt(long = "outbound_account_id")]
        outbound_account_id: Uuid,
        #[structopt(long = "inbound_uid")]
        inbound_uid: UserId,
        #[structopt(long = "inbound_account_id")]
        inbound_account_id: Uuid,
        #[structopt(short = "a", long = "amount")]
        amount: Decimal,
        #[structopt(short = "c", long = "currency")]
        currency: Currency,
    },
    RefetchNostrProfiles {
        #[structopt(short = "p", long = "pubkey")]
        pubkey: Option<String>,
        #[structopt(short = "s", long = "since")]
        since_epoch_ms: Option<u64>,
        #[structopt(short = "u", long = "until")]
        until_epoch_ms: Option<u64>,
        #[structopt(short = "l", long = "limit")]
        limit: Option<usize>,
    },
}

impl Action {
    pub fn into_request(self) -> Message {
        match self {
            Self::CreateInsuranceInvoice { amount } => {
                Message::Dealer(Dealer::CreateInsuranceInvoiceRequest(CreateInvoiceRequest {
                    req_id: Uuid::new_v4(),
                    amount,
                    memo: "ExternalDeposit".to_string(),
                }))
            }
            Self::GetBankState => {
                Message::Dealer(Dealer::BankStateRequest(BankStateRequest { req_id: Uuid::new_v4() }))
            }
            Self::MakeTx {
                outbound_uid,
                outbound_account_id,
                inbound_uid,
                inbound_account_id,
                amount,
                currency,
            } => Message::Cli(Cli::MakeTx(MakeTx {
                outbound_uid,
                outbound_account_id,
                inbound_uid,
                inbound_account_id,
                amount,
                currency,
            })),
            Self::RefetchNostrProfiles {
                pubkey,
                since_epoch_ms,
                until_epoch_ms,
                limit,
            } => Message::Nostr(Nostr::NostrProfilesRefetchRequest(NostrProfilesRefetchRequest {
                pubkey,
                since_epoch_ms,
                until_epoch_ms,
                limit,
            })),
        }
    }
}
