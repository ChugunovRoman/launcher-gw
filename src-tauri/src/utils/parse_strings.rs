use anyhow::{Result, anyhow};
use regex::Regex;
use std::sync::LazyLock;

/// Shared compiled regexes: the call sites used to recompile `Regex::new`
/// per call (and inside read_dir / org-repo loops), which is ~50µs each and
/// showed up in startup profiles.
pub static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
pub static DASHES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-]+").unwrap());

/// Parse the start offset out of a `Content-Range` header value
/// ("bytes 123-456/789" -> Some(123)). Returns None for malformed values.
pub fn parse_content_range_start(value: &str) -> Option<u64> {
  let range = value.trim().strip_prefix("bytes ")?;
  let start = range.split('-').next()?.trim();
  start.parse().ok()
}

pub fn extract_total(input: &str) -> Result<(u32, u64)> {
  let re = Regex::new(r"(\d+) files, (\d+) bytes")?;
  let captures = re.captures(input).ok_or_else(|| anyhow!("Строка не соответствует ожидаемому формату"))?;

  let files = captures.get(1).unwrap().as_str().parse::<u32>()?;
  let bytes = captures.get(2).unwrap().as_str().parse::<u64>()?;

  Ok((files, bytes))
}

pub fn extract_output(input: &str) -> Result<u64> {
  let re = Regex::new(r"Archive size: (\d+) bytes")?;
  let captures = re.captures(input).ok_or_else(|| anyhow!("Строка не соответствует ожидаемому формату"))?;

  let bytes = captures.get(1).unwrap().as_str().parse::<u64>()?;

  Ok(bytes)
}
