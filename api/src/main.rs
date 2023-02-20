use api::{start, ApiSettings};

#[actix_web::main]
async fn main() {
    let settings = utils::config::get_config_from_env::<ApiSettings>().expect("Failed to load settings.");
    let logger = utils::xlogging::init_log(settings.logging_settings());
    start(settings, logger).await.expect("Failed to start a service");
}
