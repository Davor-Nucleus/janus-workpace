use rodio::Sink;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::sync::{Arc, Mutex, mpsc};

use janus_common::logger::{log_error, log_info};

#[derive(Deserialize, Serialize)]
pub struct EnvConfig {
    #[serde(rename = "PORT_SOUNDBOARD")]
    pub port_soundboard: Option<u16>,
    #[serde(rename = "VOLUME")]
    pub volume: Option<f32>,
    #[serde(rename = "phonosCoreGui")]
    pub janus_core_gui: Option<bool>,
}

// Plus de payload pour contrôle de musique

pub struct PlayerState {
    pub stream_handle: rodio::OutputStreamHandle,
    pub volume: f32,
    // Gestion des sinks de la soundboard
    pub soundboard_sinks: Arc<Mutex<Vec<Arc<Mutex<Sink>>>>>,
    // Canaux pour arrêter les threads du soundboard
    pub soundboard_stop_channels: Arc<Mutex<Vec<std::sync::mpsc::Sender<bool>>>>,
}

impl PlayerState {
    pub fn new(stream_handle: rodio::OutputStreamHandle, initial_volume: f32) -> Self {
        Self {
            stream_handle,
            volume: initial_volume,
            soundboard_sinks: Arc::new(Mutex::new(Vec::new())),
            soundboard_stop_channels: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        // Mettre à jour le volume de tous les sinks actifs de la soundboard
        if let Ok(sinks) = self.soundboard_sinks.lock() {
            for sink_arc in sinks.iter() {
                if let Ok(s) = sink_arc.lock() {
                    s.set_volume(volume);
                }
            }
        }
        // Mettre à jour uniquement la clé VOLUME dans env.json
        if let Err(e) = update_env_key("VOLUME", serde_json::json!(volume)) {
            log_error(format!(
                "Erreur lors de la mise à jour du volume dans env.json: {}",
                e
            ));
        }
    }

    // Méthodes pour gérer les sinks de la soundboard
    pub fn add_soundboard_sink(&self, sink: Arc<Mutex<Sink>>, stop_sender: mpsc::Sender<bool>) {
        if let Ok(mut sinks) = self.soundboard_sinks.lock() {
            sinks.push(sink);
            log_info(format!(
                "Sink ajouté à la soundboard. Nombre total de sinks: {}",
                sinks.len()
            ));
        } else {
            log_error("Impossible de verrouiller la liste des sinks pour ajouter");
        }

        if let Ok(mut channels) = self.soundboard_stop_channels.lock() {
            channels.push(stop_sender);
            log_info(format!(
                "Canal d'arrêt ajouté. Nombre total de canaux: {}",
                channels.len()
            ));
        }
    }

    pub fn stop_all_soundboard_sinks(&self) {
        log_info("Tentative d'arrêt de tous les sinks du soundboard...");

        // Envoyer des signaux d'arrêt via les canaux
        if let Ok(mut channels) = self.soundboard_stop_channels.lock() {
            log_info(format!("Nombre de canaux d'arrêt: {}", channels.len()));
            for (i, sender) in channels.iter().enumerate() {
                log_info(format!("Envoi du signal d'arrêt au canal {}", i));
                if let Err(e) = sender.send(true) {
                    log_error(format!(
                        "Erreur lors de l'envoi du signal d'arrêt au canal {}: {:?}",
                        i, e
                    ));
                } else {
                    log_info(format!("Signal d'arrêt envoyé avec succès au canal {}", i));
                }
            }
            channels.clear();
            log_info("Tous les canaux d'arrêt ont été vidés");
        }

        // Attendre un peu pour que les threads se terminent
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Nettoyer les sinks restants
        if let Ok(mut sinks) = self.soundboard_sinks.lock() {
            log_info(format!("Nettoyage des sinks restants: {}", sinks.len()));
            sinks.clear();
        }

        log_info("Arrêt de la soundboard terminé");
    }

    pub fn remove_soundboard_sink(&self, sink_to_remove: &Arc<Mutex<Sink>>) {
        // Retirer un sink spécifique de la liste
        if let Ok(mut sinks) = self.soundboard_sinks.lock() {
            sinks.retain(|sink| !Arc::ptr_eq(sink, sink_to_remove));
        }
    }
}

pub fn read_env_config() -> Result<EnvConfig, Box<dyn std::error::Error>> {
    let data = fs::read_to_string("env.json")?;
    let config: EnvConfig = serde_json::from_str(&data)?;
    Ok(config)
}

pub fn write_env_config(config: &EnvConfig) -> Result<(), Box<dyn std::error::Error>> {
    let data = serde_json::to_string_pretty(config)?;
    fs::write("env.json", data)?;
    Ok(())
}

pub fn update_env_key(key: &str, value: Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut data: Value = serde_json::from_str(&fs::read_to_string("env.json")?)?;
    data[key] = value;
    fs::write("env.json", serde_json::to_string_pretty(&data)?)?;
    Ok(())
}

// Plus de découverte de dossiers de musique
