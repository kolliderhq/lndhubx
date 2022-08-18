#[derive(Debug)]
pub enum KolliderClientError {
    RequestSerializationFailed,
    WebsocketSendFailed,
    ActionTimeout,
    AuthenticationFailed,
    BalanceNotAvailable,
    NonFiatCurrency,
    CouldNotConnect,
    NotConnected,
}
