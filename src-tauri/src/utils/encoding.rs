use base64::{Engine as _, engine::general_purpose};

const KEY: &[u8] = b"my_secret_key_123";

pub fn encode(input: &str) -> String {
  let mut data = input.as_bytes().to_vec();
  for (i, byte) in data.iter_mut().enumerate() {
    *byte ^= KEY[i % KEY.len()];
  }
  general_purpose::STANDARD.encode(data)
}

pub fn decode(encoded: &str) -> Result<String, Box<dyn std::error::Error>> {
  let mut data = general_purpose::STANDARD.decode(encoded)?;
  for (i, byte) in data.iter_mut().enumerate() {
    *byte ^= KEY[i % KEY.len()];
  }
  Ok(String::from_utf8(data)?)
}
