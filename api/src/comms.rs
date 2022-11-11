use std::fmt;

use std::sync::Arc;
use std::thread;

pub use zmq::Socket as ZmqSocket;

use tokio::sync::{broadcast, mpsc, Mutex};

use msgs::*;
use utils::kafka::{Consumer, Producer};

use utils::time;

use crate::ApiSettings;

pub struct CommsActor;

pub struct Envelope {
    pub(crate) message: Message,
    pub(crate) response_tx: Option<mpsc::Sender<Result<Message, String>>>,
    pub(crate) response_filter: Option<Box<dyn Send + Fn(&Message) -> bool>>,
}

impl fmt::Debug for Envelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope").field("message", &self.message).finish()
    }
}

impl CommsActor {
    #[allow(clippy::too_many_lines)]
    pub async fn start(
        _tx: mpsc::Sender<Envelope>,
        mut rx: mpsc::Receiver<Envelope>,
        mut kafka_consumer: Consumer,
        kafka_producer: Producer,
        _api_settings: ApiSettings,
    ) {
        // users of the node actor leave their "contact details" behind so the response can be transfered back later.
        type FilterFn = Box<dyn Send + Fn(&Message) -> bool>;
        type ContactDetails = (mpsc::Sender<Result<Message, String>>, FilterFn, u64);

        // triggers garbage collection
        let filter_expiration_millis: u64 = 5000;
        let filter_size_limit: usize = 1000;

        let waiting: Mutex<Vec<ContactDetails>> = Mutex::new(Vec::with_capacity(filter_size_limit));

        let waiting = Arc::new(waiting);

        let (a_tx, mut a_rx) = broadcast::channel(1024);

        {
            let a_tx = a_tx.clone();

            thread::spawn(move || {
                while let Some(maybe_message) = kafka_consumer.consume() {
                    if let Some(message) = maybe_message {
                        let _ = a_tx.send(message);
                    };
                }
            });
        }

        let reader_task = {
            let waiting = waiting.clone();

            async move {
                while let Ok(message) = a_rx.recv().await {
                    let predicate = |(_, response_filter, _): &ContactDetails| response_filter(&message);

                    let response = |(tx, _, _): ContactDetails| {
                        let data = Ok(message.clone());
                        async move { tx.send(data).await }
                    };

                    waiting
                        .lock()
                        .await
                        // NOTE: explicit closure is needed for proper type inference apparently.
                        .drain_filter(|details| predicate(details))
                        .map(response)
                        .for_each(move |fut| {
                            tokio::spawn(fut);
                        });

                    let now = time::time_now();
                    // Only run the garbage collection when the vector is too big.
                    if waiting.lock().await.len() > filter_size_limit {
                        let _ = waiting
                            .lock()
                            .await
                            .drain_filter(|(_, _, created)| now > created as &_ + filter_expiration_millis);
                    }
                }
            }
        };

        tokio::spawn(reader_task);

        while let Some(Envelope {
            message,
            response_tx,
            response_filter,
        }) = rx.recv().await
        {
            if let (Some(tx), Some(func)) = (response_tx, response_filter) {
                waiting.lock().await.push((tx, func, time::time_now()));
            }

            kafka_producer.produce("bank", &message);
        }
    }
}
