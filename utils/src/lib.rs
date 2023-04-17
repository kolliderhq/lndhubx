pub mod config;
pub mod lnurl;
pub mod nostr;
pub mod slack;
pub mod user;
pub mod xlogging;
pub mod xzmq;

pub use url::Url;

pub mod time {
    use serde::Serializer;
    use std::time::SystemTime;

    pub const MILLISECONDS_IN_SECOND: u64 = 1000;

    pub const SECONDS_IN_MINUTE: u64 = 60;

    pub const MINUTES_IN_HOUR: u64 = 60;
    pub const SECONDS_IN_HOUR: u64 = SECONDS_IN_MINUTE * MINUTES_IN_HOUR;

    pub const HOURS_IN_DAY: u64 = 24;
    pub const MINUTES_IN_DAY: u64 = HOURS_IN_DAY * MINUTES_IN_HOUR;
    pub const SECONDS_IN_DAY: u64 = SECONDS_IN_MINUTE * MINUTES_IN_DAY;
    pub const MILLISECONDS_IN_DAY: u64 = MILLISECONDS_IN_SECOND * SECONDS_IN_DAY;

    pub fn time_now() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time should not be earlier than epoch start")
            .as_millis() as u64
    }

    #[inline]
    pub fn millis_to_nanos(millis: u64) -> u64 {
        let (result, _) = millis.overflowing_mul(1_000_000);
        result
    }

    pub fn serialize_system_time_as_millis<S>(system_time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let epoch = match system_time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_millis() as u64,
            Err(_) => 0_u64,
        };
        serializer.serialize_u64(epoch)
    }
}

pub mod currencies {
    use core_types::{Currency, Symbol};
    use std::str::FromStr;

    pub const SATS_IN_BITCOIN: u32 = 100000000;
    pub const SATS_DECIMALS: u32 = 8;

    pub fn get_base_currency_from_symbol(symbol: Symbol) -> Result<Currency, String> {
        let base = symbol[3..6].to_string();
        if let Ok(currency) = Currency::from_str(&base) {
            Ok(currency)
        } else {
            Err("Couldn't find base currency".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lnurl;

    #[test]
    fn lnurl_encode_decode() {
        let url = "https://kollider.me/.well-known/lnurlp/kollider";
        let lnurl = lnurl::encode(url, None).unwrap();
        let url_decoded = lnurl::decode(&lnurl).unwrap();
        assert_eq!(url, url_decoded);
    }
}
