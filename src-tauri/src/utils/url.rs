use url::Url;

pub fn get_base_url(full_url: &str) -> Result<String, url::ParseError> {
  let parsed = Url::parse(full_url)?;
  let base = Url::parse(&format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or("")))?.to_string();
  Ok(base)
}
