//! Configuration partagée du kit : lecture d'`env.json`, origines CORS,
//! adresse d'écoute, et écriture clé par clé.
//!
//! Toutes les briques et tous les enfants lisent le même fichier. L'écriture y est
//! donc concurrente entre processus : voir [`update_config_key`], qui fusionne sur
//! une relecture fraîche plutôt que de réécrire l'objet entier.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::net::IpAddr;
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

    #[serde(rename = "PORT_WEBRADIO")]
    pub port_webradio: Option<u16>,

    /// Volume de WebRadioCore.
    ///
    /// Clé distincte de `VOLUME` volontairement : cette dernière est déjà écrite
    /// en concurrence par JanusCore et PhonosCore, et la partager ferait que
    /// régler le volume de la radio changerait celui de la lecture locale.
    #[serde(rename = "WEBRADIO_VOLUME")]
    pub webradio_volume: Option<f32>,

    /// Interface d'écoute de WebRadioCore.
    ///
    /// `0.0.0.0` par défaut, puisque le but de la radio est d'être écoutée depuis
    /// un autre appareil du réseau local. À ramener à `127.0.0.1` pour la fermer
    /// sans recompiler : le CORS ne protège que les navigateurs, pas `curl`.
    #[serde(rename = "webRadioBind")]
    pub webradio_bind: Option<String>,

    #[serde(rename = "webRadioBitrate")]
    pub webradio_bitrate: Option<u16>,

    #[serde(rename = "webRadioCoreGui")]
    pub webradio_core_gui: Option<bool>,

    /// Pendant de `normalizationEnabled`, séparé pour la même raison que le volume.
    #[serde(rename = "webRadioNormalization")]
    pub webradio_normalization: Option<bool>,

    #[serde(rename = "PORT_ORPHEUS")]
    pub port_orpheus: Option<u16>,

    #[serde(rename = "ORPHEUS_VOLUME")]
    pub orpheus_volume: Option<f32>,

    #[serde(rename = "orpheusBind")]
    pub orpheus_bind: Option<String>,

    #[serde(rename = "orpheusBitrate")]
    pub orpheus_bitrate: Option<u16>,

    #[serde(rename = "orpheusCoreGui")]
    pub orpheus_core_gui: Option<bool>,

    /// Graine du générateur. Absente : tirée au démarrage, donc musique différente
    /// à chaque lancement. Fixée : la même session se rejoue à l'identique.
    #[serde(rename = "orpheusSeed")]
    pub orpheus_seed: Option<u64>,

    #[serde(rename = "orpheusBpm")]
    pub orpheus_bpm: Option<u16>,
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
            // 3000 = praetorcast-core, 3001 = JanusCore, 3002 = PhonosCore,
            // 3003 = chat YouTube, 3004 = présence Discord.
            port_webradio: Some(3005),
            webradio_volume: Some(1.0),
            webradio_bind: Some("0.0.0.0".to_string()),
            webradio_bitrate: Some(192),
            webradio_core_gui: Some(true),
            webradio_normalization: Some(true),
            // 3005 est pris par WebRadioCore.
            port_orpheus: Some(3006),
            orpheus_volume: Some(0.8),
            orpheus_bind: Some("0.0.0.0".to_string()),
            orpheus_bitrate: Some(192),
            orpheus_core_gui: Some(true),
            orpheus_seed: None,
            orpheus_bpm: Some(92),
        }
    }
}

impl EnvConfig {
    /// Origines autorisées en CORS : les pages servies par praetorcast-core.
    ///
    /// Mutualisé parce que c'est une **politique de sécurité**, et que les trois
    /// serveurs doivent la resserrer ou l'élargir ensemble. Avec `allow_any_origin`,
    /// n'importe quel site ouvert dans le navigateur pouvait piloter les lecteurs ;
    /// laisser trois copies de la liste, c'est risquer d'en corriger deux.
    ///
    /// Ne protège que les navigateurs : un appel `curl` n'est pas concerné.
    pub fn cors_origins(&self) -> [String; 2] {
        let port = self.port.unwrap_or(3000);
        [
            format!("http://localhost:{port}"),
            format!("http://127.0.0.1:{port}"),
        ]
    }
}

/// Interprète une adresse d'écoute, avec repli **fermé** en cas d'erreur.
///
/// Le sens du repli est le point important : une valeur illisible ramène sur la
/// boucle locale, jamais sur `0.0.0.0`. Une coquille dans `env.json` doit fermer
/// le serveur, pas l'ouvrir au réseau. Mutualisé pour cette raison — c'est une
/// décision de sécurité, et deux copies finiraient par ne plus dire la même chose.
pub fn parse_bind(setting: &str, key: &str) -> IpAddr {
    match setting.parse() {
        Ok(ip) => ip,
        Err(_) => {
            janus_log_nucleus::log_error(format!(
                "{key} « {setting} » illisible — repli sur 127.0.0.1"
            ));
            IpAddr::from([127, 0, 0, 1])
        }
    }
}

/// Écrit une clé d'`env.json` et journalise l'échec au lieu de le remonter.
///
/// Le pendant « au mieux » d'[`update_config_key`], pour les réglages dont la
/// non-persistance ne doit pas faire échouer l'action en cours : ne pas réussir à
/// enregistrer un volume n'empêche pas de l'appliquer.
pub fn persist_key(key: &str, value: Value) {
    if let Err(e) = update_config_key(key, value) {
        janus_log_nucleus::log_error(format!("Clé '{key}' non persistée dans env.json : {e}"));
    }
}

/// Read configuration from `env.json`.
/// Returns default configuration if file is missing or malformed (logging errors).
pub fn read_config() -> EnvConfig {
    let path = Path::new("env.json");
    if !path.exists() {
        janus_log_nucleus::log_info("Configuration file 'env.json' not found. Using defaults.");
        return EnvConfig::default();
    }

    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(config) => config,
            Err(e) => {
                janus_log_nucleus::log_error(format!(
                    "Error parsing 'env.json': {}. Using defaults.",
                    e
                ));
                EnvConfig::default()
            }
        },
        Err(e) => {
            janus_log_nucleus::log_error(format!("Error reading 'env.json': {}. Using defaults.", e));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_origines_cors_suivent_le_port_de_core() {
        let mut config = EnvConfig::default();
        config.port = Some(8080);
        assert_eq!(
            config.cors_origins(),
            [
                "http://localhost:8080".to_string(),
                "http://127.0.0.1:8080".to_string()
            ]
        );
    }

    #[test]
    fn une_adresse_valide_est_respectee() {
        assert_eq!(
            parse_bind("192.168.1.10", "testBind"),
            IpAddr::from([192, 168, 1, 10])
        );
        assert_eq!(parse_bind("0.0.0.0", "testBind"), IpAddr::from([0, 0, 0, 0]));
    }

    /// Le sens du repli est une décision de sécurité : une coquille doit **fermer**
    /// le serveur, jamais l'ouvrir au réseau.
    #[test]
    fn une_adresse_illisible_se_replie_sur_la_boucle_locale() {
        for mauvais in ["", "pas-une-ip", "999.1.1.1", "0.0.0.0 ", "localhost"] {
            let ip = parse_bind(mauvais, "testBind");
            assert!(ip.is_loopback(), "« {mauvais} » a donné {ip}, non fermé");
        }
    }

    /// `PORT` absent d'`env.json` est le cas courant côté JanusCore : le repli doit
    /// rester 3000, sinon les overlays se retrouvent bloqués par le CORS.
    #[test]
    fn le_port_de_core_se_replie_sur_3000() {
        let mut config = EnvConfig::default();
        config.port = None;
        assert_eq!(
            config.cors_origins(),
            [
                "http://localhost:3000".to_string(),
                "http://127.0.0.1:3000".to_string()
            ]
        );
    }
}
