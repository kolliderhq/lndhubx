mod hmac;

use core_types::{kollider_client::*, Currency, Symbol, SATS_IN_BITCOIN};
use crossbeam::channel::Sender;
use msgs::kollider_client::*;
use msgs::Message;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use url::Url;
use uuid::Uuid;
use ws_client::{Result, WsClient};
use xerror::kollider_client::*;

const WS_ACTION_TIMEOUT_SECONDS: u64 = 5;
const WS_THREAD_SLEEP_MICROSECONDS: u64 = 100;

#[derive(Debug)]
pub struct State {
    is_authenticated: bool,
    position_states: HashMap<Symbol, PositionState>,
    mark_prices: HashMap<Symbol, MarkPrice>,
    balances: Option<Balances>,
    tradable_symbols: HashMap<Symbol, TradableSymbol>,
}

impl State {
    pub fn new() -> Self {
        Self {
            is_authenticated: false,
            position_states: HashMap::new(),
            mark_prices: HashMap::new(),
            balances: None,
            tradable_symbols: HashMap::new(),
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

pub struct KolliderHedgingClient {
    state: Arc<Mutex<State>>,
    state_changed: Arc<Condvar>,
    run_flag: Arc<AtomicBool>,
    sender: Sender<String>,
    join_handle: Option<JoinHandle<()>>,
}

impl Drop for KolliderHedgingClient {
    fn drop(&mut self) {
        self.run_flag.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(join_handle) = self.join_handle.take() {
            if let Err(err) = join_handle.join() {
                eprintln!("Websocket thread had panicked, {:?}", err);
            }
        }
    }
}

impl KolliderHedgingClient {
    pub fn connect(
        url: &str,
        api_key: &str,
        api_secret: &str,
        api_passphrase: &str,
        callback: Sender<Message>,
    ) -> Result<Self> {
        let state = Arc::new(Mutex::new(State::new()));
        let state_changed = Arc::new(Condvar::new());
        let run_flag = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = crossbeam::channel::unbounded();

        let shared_state = state.clone();
        let shared_state_changed = state_changed.clone();
        let thread_run_flag = run_flag.clone();
        let ws_url = url.to_string();
        let join_handle = std::thread::spawn(move || {
            let (mut socket, _response) =
                tungstenite::connect(Url::parse(&ws_url).unwrap()).expect("Could not connect");
            {
                let stream = match socket.get_mut() {
                    MaybeTlsStream::Plain(stream) => stream,
                    // MaybeTlsStream::NativeTls(tls_stream) => tls_stream.get_ref(),
                    MaybeTlsStream::Rustls(tls_stream) => tls_stream.get_ref(),
                    _ => panic!("Unsupported stream type"),
                };

                stream
                    .set_nonblocking(true)
                    .expect("Non blocking mode could not be set");
            }
            while thread_run_flag.load(std::sync::atomic::Ordering::SeqCst) {
                let mut messages_available = false;
                if let Ok(msg) = socket.read_message() {
                    messages_available = true;
                    if let tungstenite::Message::Text(txt) = msg {
                        let response = match serde_json::from_str::<KolliderApiResponse>(&txt) {
                            Ok(deserialized) => deserialized,
                            Err(err) => {
                                eprintln!("Failed to deserialize: {}, reason: {}", txt, err);
                                continue;
                            }
                        };
                        process_incoming_message(response, &shared_state, &shared_state_changed, &callback);
                    }
                }
                if let Ok(msg) = receiver.try_recv() {
                    messages_available = true;
                    socket
                        .write_message(tungstenite::Message::Text(msg))
                        .expect("Error sending message");
                }
                if !messages_available {
                    std::thread::sleep(Duration::from_micros(WS_THREAD_SLEEP_MICROSECONDS));
                }
            }
            if let Err(err) = socket.close(None) {
                eprintln!("Failed to close a websocket gracefully, {:?}", err);
            }
        });

        let client = Self {
            state,
            state_changed,
            run_flag,
            sender,
            join_handle: Some(join_handle),
        };

        client.authenticate(api_key.to_string(), api_passphrase.to_string(), api_secret.to_string())?;
        client.fetch_tradable_symbols()?;
        client.fetch_balances()?;
        client.subscribe(
            vec![Channel::PositionStates, Channel::MarkPrices, Channel::OrderbookLevel2],
            vec![Symbol::from("BTCUSD.PERP")],
        )?;
        Ok(client)
    }

    fn authenticate(&self, token: String, passphrase: String, secret: String) -> Result<()> {
        let (timestamp, signature) = hmac::generate_authentication_signature(&secret);
        let authenticate_request = Request::Authenticate(AuthenticateRequest {
            token,
            passphrase,
            signature,
            timestamp,
        });
        self.send_request(&authenticate_request)?;
        let result = self.state_changed.wait_timeout_while(
            self.state.lock().unwrap(),
            Duration::from_secs(WS_ACTION_TIMEOUT_SECONDS),
            |state| !state.is_authenticated,
        );
        let (_lock, timeout_result) = result.map_err(|_err| KolliderClientError::AuthenticationFailed)?;
        if timeout_result.timed_out() {
            return Err(KolliderClientError::ActionTimeout);
        }
        Ok(())
    }

    fn fetch_balances(&self) -> Result<()> {
        let fetch_balances = Request::FetchBalances;
        self.send_request(&fetch_balances)
    }

    fn fetch_tradable_symbols(&self) -> Result<()> {
        let fetch_tradable_symbols = Request::FetchTradableSymbols;
        self.send_request(&fetch_tradable_symbols)
    }

    fn send_request(&self, request: &Request) -> Result<()> {
        let msg = serde_json::to_string(request).map_err(|_err| KolliderClientError::RequestSerializationFailed)?;
        self.sender
            .send(msg)
            .map_err(|_err| KolliderClientError::WebsocketSendFailed)
    }

    fn subscribe(&self, channels: Vec<Channel>, symbols: Vec<Symbol>) -> Result<()> {
        let subscription_request = Request::Subscribe(Subscribe { channels, symbols });
        self.send_request(&subscription_request)
    }

    fn order(&self, quantity: u64, currency: Currency, side: Side) -> Result<()> {
        if matches!(currency, Currency::BTC) {
            return Err(KolliderClientError::NonFiatCurrency);
        }
        let symbol: Symbol = currency.into();
        let order = Request::Order(Order::new(side, quantity, symbol, Some(Uuid::new_v4())));
        self.send_request(&order)
    }
}

impl WsClient for KolliderHedgingClient {
    fn is_authenticated(&self) -> bool {
        self.state.lock().unwrap().is_authenticated
    }

    fn get_balance(&self, currency: Currency) -> Result<Decimal> {
        if !matches!(currency, Currency::BTC) {
            let symbol: Symbol = currency.into();
            {
                let shared_state = self.state.lock().unwrap();
                let (side, upnl) = match shared_state.position_states.get(&symbol) {
                    Some(position) => match position.side.as_ref() {
                        None => {
                            return Err(KolliderClientError::BalanceNotAvailable);
                        }
                        Some(side) => (*side, position.upnl),
                    },
                    None => {
                        return Err(KolliderClientError::BalanceNotAvailable);
                    }
                };
                let mark_price = match shared_state.mark_prices.get(&symbol) {
                    None => {
                        return Err(KolliderClientError::BalanceNotAvailable);
                    }
                    Some(mark_price) => mark_price.price,
                };
                let margin = match shared_state.balances {
                    None => {
                        return Err(KolliderClientError::BalanceNotAvailable);
                    }
                    Some(ref balance) => match balance.isolated_margin.get(&symbol) {
                        Some(isolated_margin) => *isolated_margin,
                        None => {
                            return Err(KolliderClientError::BalanceNotAvailable);
                        }
                    },
                };
                let fiat_value = (margin + upnl) * mark_price / SATS_IN_BITCOIN;
                match side {
                    Side::Bid => Ok(-fiat_value),
                    Side::Ask => Ok(fiat_value),
                }
            }
        } else {
            match self.state.lock().unwrap().balances {
                Some(ref balance) => Ok(balance.cash),
                None => Err(KolliderClientError::BalanceNotAvailable),
            }
        }
    }

    fn get_all_balances(&self) -> Option<Balances> {
        self.state.lock().unwrap().balances.clone()
    }

    fn get_position_state(&self, symbol: &Symbol) -> Option<PositionState> {
        self.state.lock().unwrap().position_states.get(symbol).cloned()
    }

    fn get_tradable_symbols(&self) -> HashMap<Symbol, TradableSymbol> {
        self.state.lock().unwrap().tradable_symbols.clone()
    }

    fn make_withdrawal(&self, amount: u64, payment_request: String) -> Result<()> {
        let withdrawal_request = Request::WithdrawalRequest(WithdrawalRequest {
            withdrawal_request: WithdrawalRequestType::Ln(LnWithdrawalRequest {
                amount,
                payment_request,
            }),
        });
        self.send_request(&withdrawal_request)
    }

    fn make_order(&self, quantity: u64, symbol: Symbol, side: Side) -> Result<()> {
        let order = Request::Order(Order::new(side, quantity, symbol, Some(Uuid::new_v4())));
        self.send_request(&order)
    }

    fn buy(&self, quantity: u64, currency: Currency) -> Result<()> {
        // side is opposite because buying fiat is selling inverse contract
        self.order(quantity, currency, Side::Ask)
    }

    fn sell(&self, quantity: u64, currency: Currency) -> Result<()> {
        // side is opposite because selling fiat is buying inverse contract
        self.order(quantity, currency, Side::Bid)
    }
}

fn process_incoming_message(
    response: KolliderApiResponse,
    shared_state: &Arc<Mutex<State>>,
    shared_state_changed: &Arc<Condvar>,
    callback: &Sender<Message>,
) {
    match response.clone() {
        KolliderApiResponse::Authenticate(authenticate) => {
            if authenticate.success() {
                shared_state.lock().unwrap().is_authenticated = true;
                shared_state_changed.notify_one();
            }
        }
        KolliderApiResponse::PositionStates(position_state) => {
            shared_state
                .lock()
                .unwrap()
                .position_states
                .insert(position_state.symbol.clone(), *position_state);
            shared_state_changed.notify_one();
        }
        KolliderApiResponse::Balances(balances) => {
            shared_state.lock().unwrap().balances = Some(balances);
            shared_state_changed.notify_one();
        }
        KolliderApiResponse::MarkPrices(mark_price) => {
            shared_state
                .lock()
                .unwrap()
                .mark_prices
                .insert(mark_price.symbol.clone(), mark_price);
            shared_state_changed.notify_one();
        }
        KolliderApiResponse::OrderInvoice(_order_invoice) => {
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
        }
        KolliderApiResponse::TradableSymbols(tradable_symbols) => {
            shared_state.lock().unwrap().tradable_symbols = tradable_symbols.symbols;
            shared_state_changed.notify_one();
        }
        KolliderApiResponse::SettlementRequest(_settlement_request) => {
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
        }
        KolliderApiResponse::Level2State(_level2_state) => {
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
