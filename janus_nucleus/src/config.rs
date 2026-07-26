use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Unified configuration structure for Janus Core applications.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct EnvConfig {
    /// Port de praetorcast-core, qui sert les pages appelant ces serveurs :
    /// il détermine les origines autorisées en CORS.
    #[serde(rename = "PORT")]
    pub port: Option<u16>,

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
            port: Some(3000),
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

/// Met à jour une clé d'`env.json`, en préservant tout le reste du fichier.
///
/// Deux garde-fous, parce que ce fichier porte toute la configuration (ports,
/// identifiants Twitch, secrets Discord) et qu'un simple changement de volume
/// passe par ici :
/// - un JSON illisible fait **échouer** l'opération au lieu de repartir d'un objet
///   vide, ce qui effaçait silencieusement l'intégralité de la configuration ;
/// - l'écriture passe par un fichier temporaire renommé, pour qu'une coupure ne
///   laisse jamais un `env.json` tronqué.
pub fn update_config_key(key: &str, value: Value) -> Result<(), String> {
    let path = Path::new("env.json");

    let mut data: Value = if path.exists() {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read env.json: {}", e))?;
        serde_json::from_str(&content).map_err(|e| {
            format!("env.json est illisible ({e}) — mise à jour de '{key}' annulée pour ne pas écraser la configuration")
        })?
    } else {
        serde_json::json!({})
    };

    data[key] = value;

    let json_str = serde_json::to_string_pretty(&data)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json_str).map_err(|e| format!("Failed to write env.json: {}", e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Failed to replace env.json: {}", e)
    })
}
