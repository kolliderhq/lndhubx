pub mod config;
pub mod xzmq;
pub mod xlogging;

pub mod time {
    use serde::Serializer;
    use std::time::SystemTime;

    pub fn time_now() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
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
            Ok(duaration) => duaration.as_millis() as u64,
            Err(_) => 0_u64,
        };
        serializer.serialize_u64(epoch)
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
