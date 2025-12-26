use anyhow::{Result, anyhow};
use regex::Regex;

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
