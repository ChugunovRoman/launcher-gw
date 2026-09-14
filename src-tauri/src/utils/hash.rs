// SHA-256 helpers shared by the packer (blocking thread) and the download
// verifier (spawn_blocking).
//
// The hash is ALWAYS computed by re-reading the finished file from disk:
// - the packer's ZipWriter seeks backwards inside the archive, so a streaming
//   hash over the written bytes would be wrong;
// - download resume starts at an arbitrary Range offset, which would require
//   serializing the hasher state into the `.part` sidecar;
// - re-reading also detects on-disk corruption that happened between sessions.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// 4 MiB read buffer: large enough to keep SSDs busy, small enough not to
/// matter on the blocking-pool footprint.
const READ_BUF: usize = 4 * 1024 * 1024;

/// Compute the SHA-256 of `path` as lowercase hex (64 chars).
///
/// `on_progress(done, total)` fires after every buffer; it runs on the calling
/// (blocking) thread, so it must not do heavy work — use it for throttled
/// event emits only. `cancel` is checked between blocks: when set, hashing
/// aborts with an error instead of scanning gigabytes pointlessly.
pub fn sha256_file(
  path: &Path,
  on_progress: Option<&dyn Fn(u64, u64)>,
  cancel: Option<&AtomicBool>,
) -> Result<String> {
  let file = std::fs::File::open(path).with_context(|| format!("sha256: cannot open {}", path.display()))?;
  let total = file.metadata().map(|m| m.len()).unwrap_or(0);
  let mut reader = std::io::BufReader::with_capacity(READ_BUF, file);

  let mut hasher = Sha256::new();
  let mut buffer = vec![0u8; READ_BUF];
  let mut done: u64 = 0;

  loop {
    if let Some(flag) = cancel {
      if flag.load(std::sync::atomic::Ordering::Relaxed) {
        anyhow::bail!("sha256 of {} cancelled", path.display());
      }
    }

    let read = std::io::Read::read(&mut reader, &mut buffer)?;
    if read == 0 {
      break;
    }
    hasher.update(&buffer[..read]);
    done += read as u64;
    if let Some(cb) = on_progress {
      cb(done, total);
    }
  }

  Ok(hex_lower(&hasher.finalize()))
}

/// Lowercase hex encoding without external crates.
pub fn hex_lower(bytes: &[u8]) -> String {
  const HEX: &[u8; 16] = b"0123456789abcdef";
  let mut out = String::with_capacity(bytes.len() * 2);
  for b in bytes {
    out.push(HEX[(b >> 4) as usize] as char);
    out.push(HEX[(b & 0x0f) as usize] as char);
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sha256_of_known_vector() {
    // echo -n "abc" | sha256sum
    let dir = std::env::temp_dir().join("gw_sha_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("known.bin");
    std::fs::write(&path, b"abc").unwrap();

    let hash = sha256_file(&path, None, None).unwrap();
    assert_eq!(hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  }

  #[test]
  fn sha256_of_empty_file() {
    let dir = std::env::temp_dir().join("gw_sha_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.bin");
    std::fs::write(&path, b"").unwrap();

    let hash = sha256_file(&path, None, None).unwrap();
    assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
  }

  #[test]
  fn cancel_flag_aborts_hashing() {
    let dir = std::env::temp_dir().join("gw_sha_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cancel.bin");
    std::fs::write(&path, vec![0u8; READ_BUF * 2]).unwrap();

    let flag = AtomicBool::new(true);
    assert!(sha256_file(&path, None, Some(&flag)).is_err());
  }
}
