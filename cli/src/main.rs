use cli::cli::{Cli, CliSettings};
use structopt::StructOpt;
use utils::xzmq::SocketContext;

fn main() {
    let settings = utils::config::get_config_from_env::<CliSettings>().expect("Failed to load settings.");

    let context = SocketContext::new();
    let socket = context.create_request(&settings.bank_cli_resp_address);

    Cli::from_args().execute(socket).process_response();
}
