use slog::{o, Drain, Logger};
use serde::{Serialize, Deserialize};

pub use slog;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoggingSettings {
    pub stdout: bool,
    pub level: String,
    pub log_path: Option<String>,
    pub name: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
enum LoggignVariant {
    Stdout,
    File,
}

#[allow(clippy::collapsible_if)]
pub fn init_log(config: &LoggingSettings) -> Logger {
    let LoggingSettings {
        stdout,
        level,
        log_path,
        name,
    } = config;

    let log_path = log_path.clone().unwrap_or_else(|| String::from("/dev/null"));

    let drain_stdout_async = if *stdout {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        Some(slog_async::Async::new(drain).build().fuse())
    } else {
        None
    };

    let level = match level.as_str() {
        "debug" => slog::Level::Debug,
        "info" => slog::Level::Info,
        "error" => slog::Level::Error,
        "critical" => slog::Level::Critical,
        "trace" => slog::Level::Trace,
        st => panic!("Unknown logging level {:?}", st),
    };

    let file_drain = build_file_drain(&log_path).expect(&format!("Could not open file {}", log_path)[..]);

    if let Some(drain_stdout) = drain_stdout_async {
        // create a logger w/ both a file drain and a stdout drain
        let drain = slog::Duplicate::new(drain_stdout, file_drain).fuse();
        let filter_drain = slog::LevelFilter::new(drain, level).fuse();
        slog::Logger::root(filter_drain, o!("name" => name.to_string()))
    } else {
        // create a logger that only points to a file
        let filter_drain = slog::LevelFilter::new(file_drain, level).fuse();
        slog::Logger::root(filter_drain, o!("name" => name.to_string()))
    }
}

fn build_file_drain(log_path: &str) -> Result<slog::Fuse<slog_async::Async>, std::io::Error> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    let decorator = slog_term::PlainSyncDecorator::new(file);
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    Ok(drain)
}