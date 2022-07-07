use bech32::{self, FromBase32, ToBase32};

const PREFIX: &str = "lnurl";

pub fn encode(url: String, q: Option<String>) -> String {
    let mut unencoded = url;
    if let Some(id) = q {
        unencoded = format!("{}?q={}", unencoded, id);
    }
    let encoded = bech32::encode(PREFIX, unencoded.as_bytes().to_vec().to_base32()).unwrap();
    encoded
}

pub fn decode(encoded: String) -> String {
    let (_hrp, data) = bech32::decode(&encoded).unwrap();
    let base_32 = Vec::<u8>::from_base32(&data).unwrap();
    std::str::from_utf8(&base_32).unwrap().to_string()
}
