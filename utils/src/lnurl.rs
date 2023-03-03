use bech32::{self, FromBase32, ToBase32};

const PREFIX: &str = "lnurl";

pub fn encode(base_url: &str, q: Option<String>) -> Result<String, bech32::Error> {
    let url = match q {
        Some(id) => format!("{base_url}?q={id}"),
        None => base_url.to_string(),
    };
    let validated_url = url::Url::parse(&url)
        .map_err(|_| bech32::Error::InvalidData(0))?
        .to_string();
    bech32::encode(PREFIX, validated_url.as_bytes().to_base32())
}

pub fn decode(encoded: &str) -> Result<String, bech32::Error> {
    let (_hrp, data) = bech32::decode(encoded)?;
    let base_32 = Vec::<u8>::from_base32(&data)?;
    let str = std::str::from_utf8(&base_32).map_err(|_| bech32::Error::InvalidData(0))?;
    let validated_url = url::Url::parse(str)
        .map_err(|_| bech32::Error::InvalidData(0))?
        .to_string();
    Ok(validated_url)
}
