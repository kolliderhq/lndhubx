use cli::cli::{Cli, CliSettings};
use structopt::StructOpt;
use utils::kafka::{Consumer, Producer};

fn main() {
    let settings = utils::config::get_config_from_env::<CliSettings>().expect("Failed to load settings.");
    let producer = Producer::new(&settings.kafka_broker_addresses);
    let consumer = Consumer::new("cli", "cli", &settings.kafka_broker_addresses);
    Cli::from_args().execute(producer, consumer).process_response();
}
