// use log::LevelFilter;
// use log4rs::append::file::FileAppender;
// use log4rs::append::console::ConsoleAppender;
// use log4rs::encode::pattern::PatternEncoder;
// use log4rs::config::{Appender, Config, Root};

pub fn create_logger() -> Result<(), String> {
    // let stdout = ConsoleAppender::builder()
    //     .encoder(Box::new(PatternEncoder::new("[{h({l})}] {d(%Y-%m-%d %H:%M:%S %Z)(utc)} - {m}{n}")))
    //     .build();

    // let config = Config::builder()
    //     .appender(Appender::builder().build("stdout", Box::new(stdout)))
    //     .build(Root::builder().appender("stdout").build(LevelFilter::Debug))
    //     .unwrap();
    // let handle = log4rs::init_config(config);

    Ok(())
}
