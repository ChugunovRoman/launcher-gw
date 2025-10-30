use base64::{Engine as _, engine::general_purpose};

const KEY: &[u8] = b"my_secret_key_123";

fn scramble(bytes: &mut [u8]) {
  for i in 0..bytes.len() {
    bytes.swap(i, (i * 7 + 13) % bytes.len());
  }
}

fn unscramble(bytes: &mut [u8]) {
  let mut indices = (0..bytes.len()).collect::<Vec<_>>();
  for i in 0..bytes.len() {
    indices[i] = (i * 7 + 13) % bytes.len();
  }
  let mut rev = vec![0; bytes.len()];
  for (i, &idx) in indices.iter().enumerate() {
    rev[idx] = i;
  }

  let mut temp = bytes.to_vec();
  for i in 0..bytes.len() {
    bytes[i] = temp[rev[i]];
  }
}

fn encode(input: &str) -> String {
  let mut data = input.as_bytes().to_vec();

  for (i, byte) in data.iter_mut().enumerate() {
    *byte ^= KEY[i % KEY.len()];
  }

  scramble(&mut data);

  general_purpose::STANDARD.encode(data)
}

fn decode(encoded: &str) -> Result<String, Box<dyn std::error::Error>> {
  let mut data = general_purpose::STANDARD.decode(encoded)?;

  unscramble(&mut data);

  for (i, byte) in data.iter_mut().enumerate() {
    *byte ^= KEY[i % KEY.len()];
  }

  Ok(String::from_utf8(data)?)
}

// fn main() -> Result<(), Box<dyn std::error::Error>> {
//   let original = "Привет, мир! 🌍";
//   let encoded = encode(original);
//   println!("Encoded: {}", encoded);

//   std::fs::write("secret.txt", &encoded)?;

//   let read_encoded = std::fs::read_to_string("secret.txt")?;
//   let decoded = decode(&read_encoded)?;
//   println!("Decoded: {}", decoded);

//   assert_eq!(original, decoded);
//   Ok(())
// }
