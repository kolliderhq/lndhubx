use std::env;

pub fn get_config_from_env<'a, T: 'a>() -> Result<T, config::ConfigError>
where
    T: serde::Deserialize<'a>,
{
    let environment: String = env::var("ENV").unwrap_or_else(|_| "dev".into());
    let file_name: String = env::var("FILE_NAME").expect("FILE_NAME was not specified as an environment variable.");

    let file_path = format!("{}.{}.toml", file_name, environment);

    let mut configuration = config::Config::default();
    configuration.merge(config::File::with_name(&file_path))?;
    configuration.try_into()
}
