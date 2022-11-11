use rdkafka::{
    config::ClientConfig,
    consumer::{BaseConsumer, Consumer as KafkaConsumer},
    producer::{BaseRecord, ProducerContext, ThreadedProducer},
    ClientContext, Message as KafkaMessage,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;

const KEY: &str = "lndhubx_msg";

struct ProduceCallbackLogger;

impl ClientContext for ProduceCallbackLogger {}

impl ProducerContext for ProduceCallbackLogger {
    type DeliveryOpaque = ();

    fn delivery(
        &self,
        delivery_result: &rdkafka::producer::DeliveryResult<'_>,
        _delivery_opaque: Self::DeliveryOpaque,
    ) {
        let dr = delivery_result.as_ref();

        if let Err(producer_err) = dr {
            let key: &str = producer_err.1.key_view().unwrap().unwrap();
            println!("failed to produce message with key {} - {}", key, producer_err.0,)
        }
    }
}

pub struct Producer {
    producer: ThreadedProducer<ProduceCallbackLogger>,
}

impl Producer {
    pub fn new(broker_servers: &str) -> Self {
        let producer: ThreadedProducer<ProduceCallbackLogger> = ClientConfig::new()
            .set("bootstrap.servers", broker_servers)
            .set("message.timeout.ms", "5000")
            .set("batch.size", "1")
            .set("linger.ms", "0")
            .set("max.in.flight.requests.per.connection", "1")
            .set("acks", "all")
            .create_with_context(ProduceCallbackLogger {})
            .expect("invalid producer config");
        Self { producer }
    }

    pub fn produce<T>(&self, topic: &str, msg: &T)
    where
        T: Serialize + Debug,
    {
        let payload = serde_json::to_string(msg).unwrap_or_else(|err| {
            panic!("Failed to serialize message {:?}, err: {:?}", msg, err);
        });
        if let Err(err) = self.producer.send(BaseRecord::to(topic).payload(&payload).key(KEY)) {
            eprintln!(
                "Failed to send a message: {} into {} topic over kafka, error: {:?}",
                payload, topic, err
            );
        } else {
            eprintln!("===mk=== Sent {} into {} topic", payload, topic);
        }
    }
}

pub struct Consumer {
    topic: String,
    consumer: BaseConsumer,
    last_offset: i64,
}

impl Consumer {
    pub fn new(group_id: &str, topic: &str, broker_servers: &str) -> Self {
        let consumer: BaseConsumer = ClientConfig::new()
            .set("bootstrap.servers", broker_servers)
            .set("group.id", group_id)
            .set("enable.partition.eof", "false")
            .create()
            .expect("invalid consumer config");
        consumer.subscribe(&[topic]).expect("topic subscribe failed");
        Self {
            topic: topic.to_string(),
            consumer,
            last_offset: 0,
        }
    }

    pub fn consume<T: DeserializeOwned>(&mut self) -> Option<Option<T>> {
        eprintln!("===mk=== Consume from {} kafka topic", self.topic);
        let msg_result = self.consumer.iter().next()?;
        eprintln!("===mk=== Consumed from {} kafka topic", self.topic);
        let msg = msg_result.unwrap();
        let topic = msg.topic();
        if topic != self.topic {
            panic!("Topic mismatch: expected {}, got {}", self.topic, topic)
        }
        let key: &str = msg.key_view().unwrap().unwrap();
        let value = msg.payload().unwrap();
        let offset = msg.offset();
        if offset != 0 && offset <= self.last_offset {
            return Some(None);
        }
        self.last_offset = offset;
        if key != KEY {
            return Some(None);
        }
        match serde_json::from_slice::<T>(value) {
            Ok(msg) => Some(Some(msg)),
            Err(err) => {
                eprintln!("Failed to deserialize {:?} into message, err: {:?}", value, err);
                Some(None)
            }
        }
    }
}
