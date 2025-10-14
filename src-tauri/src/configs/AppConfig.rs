use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
  pub app_name: String,
  pub version: String,
  pub debug: bool,
}

impl AppConfig {
  pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
    let content = fs::read_to_string("config.json")?;
    let config: Self = serde_json::from_str(&content)?;
    Ok(config)
  }

  pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
    let content = serde_json::to_string_pretty(self)?;
    fs::write("config.json", content)?;
    Ok(())
  }
}
