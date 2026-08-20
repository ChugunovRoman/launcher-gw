use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose};
use encoding_rs::WINDOWS_1251;
use std::{fs, path::Path};

// ---------------------------------------------------------------------------
// Provider token storage
//
// Legacy format: XOR with a compile-time key + base64 — obfuscation only,
// anyone with config.json and the sources can recover the token.
// Current format (Windows): "dpapi1:" + base64(CryptProtectData(...)) bound
// to the current Windows user via DPAPI. Legacy values are transparently
// migrated on startup (see setup.rs).
// ---------------------------------------------------------------------------

/// Prefix marking a DPAPI-encrypted token value.
const TOKEN_PREFIX_DPAPI: &str = "dpapi1:";

/// Legacy XOR key — kept ONLY to read/migrate already-stored tokens.
const LEGACY_KEY: &[u8] = b"my_secret_key_123";

fn legacy_encode(input: &str) -> String {
  let mut data = input.as_bytes().to_vec();
  for (i, byte) in data.iter_mut().enumerate() {
    *byte ^= LEGACY_KEY[i % LEGACY_KEY.len()];
  }
  general_purpose::STANDARD.encode(data)
}

fn legacy_decode(encoded: &str) -> Result<String> {
  let mut data = general_purpose::STANDARD.decode(encoded)?;
  for (i, byte) in data.iter_mut().enumerate() {
    *byte ^= LEGACY_KEY[i % LEGACY_KEY.len()];
  }
  Ok(String::from_utf8(data)?)
}

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>> {
  use windows::core::PCWSTR;
  use windows::Win32::Foundation::{HLOCAL, LocalFree};
  use windows::Win32::Security::Cryptography::{CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptProtectData};

  unsafe {
    let in_blob = CRYPT_INTEGER_BLOB {
      cbData: plain.len() as u32,
      pbData: plain.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    CryptProtectData(
      &in_blob,
      PCWSTR::null(),
      None,
      None,
      None,
      CRYPTPROTECT_UI_FORBIDDEN,
      &mut out_blob,
    )?;

    let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out_blob.pbData as _)));
    Ok(out)
  }
}

#[cfg(windows)]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
  use windows::Win32::Foundation::{HLOCAL, LocalFree};
  use windows::Win32::Security::Cryptography::{CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptUnprotectData};

  unsafe {
    let in_blob = CRYPT_INTEGER_BLOB {
      cbData: cipher.len() as u32,
      pbData: cipher.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    CryptUnprotectData(
      &in_blob,
      None,
      None,
      None,
      None,
      CRYPTPROTECT_UI_FORBIDDEN,
      &mut out_blob,
    )?;

    let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out_blob.pbData as _)));
    Ok(out)
  }
}

/// Encode a token for storage in config.json. Empty token stays empty.
pub fn encode_token(plain: &str) -> String {
  if plain.is_empty() {
    return String::new();
  }

  #[cfg(windows)]
  {
    match dpapi_protect(plain.as_bytes()) {
      Ok(cipher) => return format!("{}{}", TOKEN_PREFIX_DPAPI, general_purpose::STANDARD.encode(cipher)),
      Err(e) => log::warn!("DPAPI protect failed, falling back to legacy token encoding: {}", e),
    }
  }

  legacy_encode(plain)
}

/// Decode a stored token; understands both DPAPI and legacy formats.
pub fn decode_token(stored: &str) -> Result<String> {
  if stored.is_empty() {
    return Ok(String::new());
  }

  if let Some(payload) = stored.strip_prefix(TOKEN_PREFIX_DPAPI) {
    #[cfg(windows)]
    {
      let cipher = general_purpose::STANDARD.decode(payload)?;
      return Ok(String::from_utf8(dpapi_unprotect(&cipher)?)?);
    }
    #[cfg(not(windows))]
    {
      let _ = payload;
      bail!("cannot decode a DPAPI-stored token on this platform");
    }
  }

  legacy_decode(stored)
}

/// True when the stored value still uses the legacy XOR format and should be
/// migrated to the DPAPI-backed storage.
pub fn is_legacy_token(stored: &str) -> bool {
  !stored.is_empty() && !stored.starts_with(TOKEN_PREFIX_DPAPI)
}

/// Mask a plain token for display: never expose the token itself to the UI.
pub fn mask_token(plain: &str) -> String {
  const VISIBLE_TAIL: usize = 4;
  let char_count = plain.chars().count();
  if char_count == 0 {
    return String::new();
  }
  if char_count <= VISIBLE_TAIL {
    return "••••".to_string();
  }
  let tail: String = plain.chars().rev().take(VISIBLE_TAIL).collect::<Vec<_>>().into_iter().rev().collect();
  format!("••••••••{}", tail)
}

pub fn read_cp1251_file<P: AsRef<Path>>(path: P) -> Result<String> {
  let bytes = fs::read(path)?;

  let (res, _encoding_used, has_errors) = WINDOWS_1251.decode(&bytes);

  if has_errors {
    bail!("Ошибка при декодировании: файл содержит некорректные символы");
  }

  Ok(res.into_owned())
}
