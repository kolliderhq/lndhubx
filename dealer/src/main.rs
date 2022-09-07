pub mod dealer_engine;

use utils::xzmq::SocketContext;

use dealer::dealer_engine::*;
use dealer::start;

#[tokio::main]
async fn main() {
    let settings = utils::config::get_config_from_env::<DealerEngineSettings>().expect("Failed to load settings.");

    let context = SocketContext::new();
    let bank_rx = context.create_pull(&settings.dealer_bank_pull_address);
    let bank_tx = context.create_push(&settings.dealer_bank_push_address);

    start(settings, bank_tx, bank_rx).await;
}
