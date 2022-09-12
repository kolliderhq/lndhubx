use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::SystemTime;

type HmacSha256 = Hmac<Sha256>;

pub fn generate_authentication_signature(b64_secret: &str) -> Result<(String, String), ()> {
    let timestamp_str = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let mut message = timestamp_str.clone();
    message.push_str("authentication");
    match hmac_encrypt_message(b64_secret, &message) {
        Ok(encrypted_message) => Ok((timestamp_str, encrypted_message)),
        Err(_) => Err(()),
    }
}

fn hmac_encrypt_message(b64_secret: &str, message: &str) -> Result<String, ()> {
    let decoded_secret = base64::decode(&b64_secret).map_err(|_| ())?;
    let mut mac = HmacSha256::new_from_slice(&decoded_secret).map_err(|_| ())?;
    mac.update(message.as_bytes());
    let result = mac.finalize();
    let code_bytes = result.into_bytes();
    Ok(base64::encode(&code_bytes))
}
