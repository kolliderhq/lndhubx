use std::any::Any;
use std::sync::{
    mpsc::{channel, Receiver, SendError, Sender},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};

pub use slack_hook::{PayloadBuilder, Slack, SlackText};

use slog::{Level, OwnedKVList, Record};

#[derive(Debug)]
pub(crate) struct SlackDrain {
    inner: Arc<Mutex<SlackBot>>,
}

impl slog::Drain for SlackDrain {
    type Ok = ();
    type Err = ();

    fn log(&self, record: &Record, _: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        if matches!(record.level(), Level::Warning | Level::Error | Level::Critical) {
            let inner = self.inner.lock().unwrap();
            let payload = PayloadBuilder::new().text(format!(
                "Service Name: *{}* \n ----------------------------- \n *{}*",
                &inner.service_name,
                record.msg()
            ));
            let _ = inner.tx.send(payload);
        }

        Ok(())
    }
}

impl SlackDrain {
    pub fn new_with_hook(url: &str, chan: &str, service_name: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SlackBot::new(url, chan, service_name))),
        }
    }
}

#[derive(Debug)]
pub struct SlackBot {
    tx: Sender<PayloadBuilder>,
    handle: JoinHandle<()>,
    service_name: String,
}

impl SlackBot {
    pub fn new(hook: &str, chan: &str, service_name: &str) -> Self {
        let (tx, rx) = channel();
        let slack = Slack::new(hook).expect("Invalid hook url");
        let chan = chan.to_string();
        Self {
            tx,
            handle: thread::spawn(move || Self::handle(slack, rx, chan)),
            service_name: service_name.to_string(),
        }
    }

    fn handle(slack: Slack, rx: Receiver<PayloadBuilder>, channel: String) {
        while let Ok(msg) = rx.recv() {
            if let Err(e) = msg.channel(channel.clone()).build().and_then(|ref x| slack.send(x)) {
                eprintln!("[SLACK] {:?}", e);
            }
        }
    }

    pub fn tx(&self) -> SlackChannel {
        SlackChannel { tx: self.tx.clone() }
    }

    pub fn join(self) -> Result<(), Box<(dyn Any + Send)>> {
        self.handle.join()
    }
}

#[derive(Clone)]
pub struct SlackChannel {
    tx: Sender<PayloadBuilder>,
}

impl SlackChannel {
    pub fn send<T: Into<SlackText>>(&mut self, msg: T) -> Result<(), Box<SendError<PayloadBuilder>>> {
        let payload = PayloadBuilder::new().text(msg.into());
        self.tx.send(payload).map_err(Box::new)
    }

    pub fn send_raw(&mut self, payload: PayloadBuilder) -> Result<(), Box<SendError<PayloadBuilder>>> {
        self.tx.send(payload).map_err(Box::new)
    }
}
