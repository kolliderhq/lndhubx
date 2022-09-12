pub mod config;
pub mod lnurl;
pub mod xlogging;
pub mod xzmq;

pub mod time {
    use serde::Serializer;
    use std::time::SystemTime;

    pub fn time_now() -> u64 {
        match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_millis() as u64,
            Err(err) => {
                eprintln!("Failed to get current system time as epoch, reason: {:?}", err);
                panic!("Failed to get current system time as epoch");
            }
        }
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
            Ok(duaration) => duaration.as_millis() as u64,
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
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
