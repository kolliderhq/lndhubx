use bech32::{self, FromBase32, ToBase32};

const PREFIX: &str = "lnurl";

pub fn encode(url: String, q: Option<String>) -> Result<String, bech32::Error> {
    let mut unencoded = url;
    if let Some(id) = q {
        unencoded = format!("{}?q={}", unencoded, id);
    }
    bech32::encode(PREFIX, unencoded.as_bytes().to_vec().to_base32())
}

pub fn decode(encoded: String) -> Result<String, bech32::Error> {
    let (_hrp, data) = bech32::decode(&encoded)?;
    let base_32 = Vec::<u8>::from_base32(&data)?;
    let str = std::str::from_utf8(&base_32).map_err(|_| bech32::Error::InvalidData(0))?;
    Ok(str.to_string())
}
