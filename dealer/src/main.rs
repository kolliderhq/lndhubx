pub mod dealer_engine;

use utils::xzmq::{create_pull, create_push};

use dealer::dealer_engine::*;
use dealer::start;

fn main() {
    let settings = utils::config::get_config_from_env::<DealerEngineSettings>().expect("Failed to load settings.");

    let bank_rx = create_pull(&settings.dealer_bank_pull_address);
    let bank_tx = create_push(&settings.dealer_bank_push_address);

    start(settings, bank_tx, bank_rx);
}
