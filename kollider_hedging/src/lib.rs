mod hmac;

use core_types::{kollider_client::*, Currency, Symbol, SATS_IN_BITCOIN};
use crossbeam::channel::Sender;
use msgs::kollider_client::*;
use msgs::Message;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;
use url::Url;
use uuid::Uuid;
use ws_client::{Result, WsClient};
use xerror::kollider_client::*;

const WS_ACTION_TIMEOUT_SECONDS: u64 = 5;
const WS_THREAD_SLEEP_MICROSECONDS: u64 = 100;
const WS_THREAD_RECONNECT_MILLISECONDS: u64 = 5000;

#[derive(Debug)]
pub struct State {
    is_connected: bool,
    is_authenticated: bool,
    has_received_positions: bool,
    position_states: HashMap<Symbol, PositionState>,
    mark_prices: HashMap<Symbol, MarkPrice>,
    balances: Option<Balances>,
    tradable_symbols: HashMap<Symbol, TradableSymbol>,
}

impl State {
    pub fn new() -> Self {
        Self {
            is_connected: false,
            is_authenticated: false,
            has_received_positions: false,
            position_states: HashMap::new(),
            mark_prices: HashMap::new(),
            balances: None,
            tradable_symbols: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

pub struct KolliderHedgingClient {
    api_key: String,
    api_secret: String,
    api_passphrase: String,
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
        let ws_url = Url::parse(url).expect("Could not parse url");
        let mut socket = Self::open_socket(ws_url.clone())?;
        shared_state.lock().unwrap().is_connected = true;
        let join_handle = std::thread::spawn(move || {
            while thread_run_flag.load(std::sync::atomic::Ordering::SeqCst) {
                let mut messages_available = false;
                if !socket.can_read() {
                    if is_connected(&shared_state) {
                        set_disconnected(&shared_state, &shared_state_changed, &callback);
                    }
                    match Self::open_socket(ws_url.clone()) {
                        Ok(new_socket) => {
                            socket = new_socket;
                            set_reconnected(&shared_state, &shared_state_changed, &callback);
                        }
                        Err(_err) => {
                            std::thread::sleep(Duration::from_millis(WS_THREAD_RECONNECT_MILLISECONDS));
                        }
                    }
                    continue;
                }
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
                if !socket.can_write() {
                    if is_connected(&shared_state) {
                        set_disconnected(&shared_state, &shared_state_changed, &callback);
                    }
                    match Self::open_socket(ws_url.clone()) {
                        Ok(new_socket) => socket = new_socket,
                        Err(_err) => {
                            std::thread::sleep(Duration::from_millis(WS_THREAD_RECONNECT_MILLISECONDS));
                        }
                    }
                    continue;
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
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            api_passphrase: api_passphrase.to_string(),
            state,
            state_changed,
            run_flag,
            sender,
            join_handle: Some(join_handle),
        };

        client.initialise()?;
        Ok(client)
    }

    fn open_socket(url: Url) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
        let (mut socket, _response) = tungstenite::connect(url).map_err(|_| KolliderClientError::CouldNotConnect)?;
        let stream = match socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream,
            //MaybeTlsStream::NativeTls(tls_stream) => tls_stream.get_ref(),
            MaybeTlsStream::Rustls(tls_stream) => tls_stream.get_ref(),
            _ => panic!("Unsupported stream type"),
        };

        stream
            .set_nonblocking(true)
            .expect("Non blocking mode could not be set");
        Ok(socket)
    }

    fn initialise(&self) -> Result<()> {
        self.authenticate(
            self.api_key.clone(),
            self.api_passphrase.clone(),
            self.api_secret.clone(),
        )?;
        self.fetch_tradable_symbols()?;
        self.fetch_positions()?;
        self.fetch_balances()
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
        self.checked_send_request(&fetch_balances)
    }

    fn fetch_tradable_symbols(&self) -> Result<()> {
        let fetch_tradable_symbols = Request::FetchTradableSymbols;
        self.checked_send_request(&fetch_tradable_symbols)
    }

    fn fetch_positions(&self) -> Result<()> {
        let fetch_positions = Request::FetchPositions;
        self.checked_send_request(&fetch_positions)
    }

    fn checked_send_request(&self, request: &Request) -> Result<()> {
        if !self.is_connected() {
            return Err(KolliderClientError::NotConnected);
        }
        if !self.is_authenticated() {
            self.initialise()?;
        }
        self.send_request(request)
    }

    fn send_request(&self, request: &Request) -> Result<()> {
        if !self.is_connected() {
            return Err(KolliderClientError::NotConnected);
        }
        let msg = serde_json::to_string(request).map_err(|_err| KolliderClientError::RequestSerializationFailed)?;
        self.sender
            .send(msg)
            .map_err(|_err| KolliderClientError::WebsocketSendFailed)
    }

    pub fn subscribe(&self, channels: Vec<Channel>, symbols: Vec<Symbol>) -> Result<()> {
        let subscription_request = Request::Subscribe(Subscribe { channels, symbols });
        self.checked_send_request(&subscription_request)
    }

    pub fn subscribe_all(&self, channels: Vec<Channel>) -> Result<()> {
        let symbols = vec![];
        let subscription_request = Request::Subscribe(Subscribe { channels, symbols });
        self.checked_send_request(&subscription_request)
    }

    fn order(&self, quantity: u64, currency: Currency, side: Side) -> Result<()> {
        if matches!(currency, Currency::BTC) {
            return Err(KolliderClientError::NonFiatCurrency);
        }
        let symbol: Symbol = currency.into();
        let order = Request::Order(Order::new(side, quantity, symbol, Some(Uuid::new_v4())));
        self.checked_send_request(&order)
    }
}

impl WsClient for KolliderHedgingClient {
    fn is_authenticated(&self) -> bool {
        self.state.lock().unwrap().is_authenticated
    }

    fn is_connected(&self) -> bool {
        self.state.lock().unwrap().is_connected
    }

    fn is_ready(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.is_connected && state.is_authenticated
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
                Some(ref balance) => {
                    let symbol = Symbol::from("SAT");
                    let sats_balance = balance.cash.get(&symbol).unwrap();
                    Ok(*sats_balance)
                }
                None => Err(KolliderClientError::BalanceNotAvailable),
            }
        }
    }

    fn subscribe(&self, channels: Vec<Channel>, symbols: Option<Vec<Symbol>>) {
        if let Some(s) = symbols {
            self.subscribe(channels, s).unwrap();
        } else {
            self.subscribe_all(channels).unwrap();
        }
    }

    fn get_all_balances(&self) -> Option<Balances> {
        self.state.lock().unwrap().balances.clone()
    }

    fn get_position_state(&self, symbol: &Symbol) -> Result<Option<PositionState>> {
        let shared_state = self.state.lock().unwrap();
        if shared_state.has_received_positions {
            Ok(shared_state.position_states.get(symbol).cloned())
        } else {
            Err(KolliderClientError::PositionStateNotAvailable)
        }
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
        self.checked_send_request(&withdrawal_request)
    }

    fn make_order(&self, quantity: u64, symbol: Symbol, side: Side) -> Result<()> {
        let order = Request::Order(Order::new(side, quantity, symbol, Some(Uuid::new_v4())));
        self.checked_send_request(&order)
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

fn is_connected(shared_state: &Arc<Mutex<State>>) -> bool {
    shared_state.lock().unwrap().is_connected
}

fn set_disconnected(shared_state: &Arc<Mutex<State>>, shared_state_changed: &Arc<Condvar>, callback: &Sender<Message>) {
    let mut state = shared_state.lock().unwrap();
    state.clear();
    shared_state_changed.notify_one();
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let msg = Message::KolliderApiResponse(KolliderApiResponse::Disconnected(Disconnected { timestamp }));
    callback.send(msg).unwrap();
}

fn set_reconnected(shared_state: &Arc<Mutex<State>>, shared_state_changed: &Arc<Condvar>, callback: &Sender<Message>) {
    let mut state = shared_state.lock().unwrap();
    state.is_authenticated = false;
    state.is_connected = true;
    shared_state_changed.notify_one();
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let msg = Message::KolliderApiResponse(KolliderApiResponse::Reconnected(Reconnected { timestamp }));
    callback.send(msg).unwrap();
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
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
        }
        KolliderApiResponse::PositionStates(position_state) => {
            {
                let mut shared_state = shared_state.lock().unwrap();
                shared_state
                    .position_states
                    .insert(position_state.symbol.clone(), *position_state);
            }
            shared_state_changed.notify_one();
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
        }
        KolliderApiResponse::Positions(positions) => {
            {
                let mut shared_state = shared_state.lock().unwrap();
                shared_state.position_states = positions.positions;
                shared_state.has_received_positions = true;
            }
            shared_state_changed.notify_one();
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
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
            let msg = Message::KolliderApiResponse(response);
            callback.send(msg).unwrap();
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
