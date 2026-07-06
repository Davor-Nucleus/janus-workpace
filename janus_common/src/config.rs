use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Unified configuration structure for Janus Core applications.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct EnvConfig {
    #[serde(rename = "PORT_MUSIC")]
    pub port_music: Option<u16>,

    #[serde(rename = "PORT_SOUNDBOARD")]
    pub port_soundboard: Option<u16>,

    #[serde(rename = "VOLUME")]
    pub volume: Option<f32>,

    #[serde(rename = "janusCoreGui")]
    pub janus_core_gui: Option<bool>,

    #[serde(rename = "phonosCoreGui")]
    pub phonos_core_gui: Option<bool>,

    #[serde(rename = "normalizationEnabled")]
    pub normalization_enabled: Option<bool>,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            port_music: Some(3030),
            port_soundboard: Some(3003),
            volume: Some(1.0),
            janus_core_gui: Some(true),
            phonos_core_gui: Some(true),
            normalization_enabled: Some(true),
        }
    }
}

/// Read configuration from `env.json`.
/// Returns default configuration if file is missing or malformed (logging errors).
pub fn read_config() -> EnvConfig {
    let path = Path::new("env.json");
    if !path.exists() {
        crate::logger::log_info("Configuration file 'env.json' not found. Using defaults.");
        return EnvConfig::default();
    }

    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(config) => config,
            Err(e) => {
                crate::logger::log_error(format!(
                    "Error parsing 'env.json': {}. Using defaults.",
                    e
                ));
                EnvConfig::default()
            }
        },
        Err(e) => {
            crate::logger::log_error(format!("Error reading 'env.json': {}. Using defaults.", e));
            EnvConfig::default()
        }
    }
}

/// Update a specific key in `env.json`.
/// Creates the file if it doesn't exist.
pub fn update_config_key(key: &str, value: Value) -> Result<(), String> {
    let path = Path::new("env.json");

    // Read existing or create empty object
    let mut data: Value = if path.exists() {
        match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(e) => return Err(format!("Failed to read env.json: {}", e)),
        }
    } else {
        serde_json::json!({})
    };

    // Update key
    data[key] = value;

    // Write back
    match serde_json::to_string_pretty(&data) {
        Ok(json_str) => match fs::write(path, json_str) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to write env.json: {}", e)),
        },
        Err(e) => Err(format!("Failed to serialize config: {}", e)),
    }
}
