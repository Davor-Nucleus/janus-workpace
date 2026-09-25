use rodio::{Decoder, Sink};
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc},
    time::Duration,
};
use warp::Reply;

use crate::model::{ActiveSound, PlayerState};
use janus_log_nucleus::{log_error, log_info};
use janus_platform_nucleus::paths::resolve_within;

/// Délai maximal d'un appel à JanusCore. Injoignable ou figé, il ne doit bloquer ni
/// un son ni un handler : sans délai, reqwest attend indéfiniment.
const MUSIC_TIMEOUT: Duration = Duration::from_secs(2);

pub struct PlayerController;

impl PlayerController {
    pub async fn handle_soundboard_play(
        params: HashMap<String, String>,
        player: Arc<Mutex<PlayerState>>,
        music_port: u16,
    ) -> Result<impl Reply, std::convert::Infallible> {
        // Le son est résolu **avant** de toucher à la musique : un nom manquant ou
        // inconnu répondait 400 en laissant JanusCore en pause.
        let Some(sound_name) = params.get("sound") else {
            return Ok(warp::reply::with_status(
                "Paramètre 'sound' manquant".to_string(),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        };
        let Some(file_path) = resolve_sound(sound_name) else {
            return Ok(warp::reply::with_status(
                "Son introuvable dans ./public/soundboard".to_string(),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        };

        // Seul le premier son d'une série décide de la pause (cf. `MusicHold`).
        let first = lock(&player).hold.begin();
        if first && music_is_playing(music_port).await {
            let _ = http().get(music_url(music_port, "pause")).send().await;
            lock(&player).hold.set_resume(true);
        }

        let reply = format!("Son {:?} joué", file_path.file_name().unwrap_or_default());

        // Joue le son du soundboard (via tokio spawn_blocking pour ne pas bloquer le runtime)
        tokio::task::spawn_blocking(move || {
            play_blocking(&file_path, &player);
            // Toujours appelé, y compris quand la lecture a échoué avant de
            // commencer : sinon le compteur de sons ne redescendrait jamais.
            finish_sound(&player, music_port);
        });

        Ok(warp::reply::with_status(reply, warp::http::StatusCode::OK))
    }

    pub async fn handle_soundboard_sounds() -> Result<impl Reply, std::convert::Infallible> {
        let base = Path::new("./public/soundboard");

        if !base.exists() || !base.is_dir() {
            let response = serde_json::json!({
                "error": "Le dossier soundboard n'existe pas"
            });
            return Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::NOT_FOUND,
            ));
        }

        let mut sounds = Vec::new();

        match std::fs::read_dir(base) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() {
                            // Vérifie si c'est un fichier audio supporté
                            if let Some(extension) = path.extension() {
                                if let Some(ext_str) = extension.to_str() {
                                    if ["mp3", "wav", "flac", "ogg", "m4a"]
                                        .contains(&ext_str.to_lowercase().as_str())
                                    {
                                        if let Some(file_name) = path.file_name() {
                                            if let Some(name_str) = file_name.to_str() {
                                                sounds.push(name_str.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let response = serde_json::json!({ "sounds": sounds });
                Ok(warp::reply::with_status(
                    warp::reply::json(&response),
                    warp::http::StatusCode::OK,
                ))
            }
            Err(e) => {
                let response = serde_json::json!({
                    "error": format!("Erreur lors de la lecture du dossier soundboard: {}", e)
                });
                Ok(warp::reply::with_status(
                    warp::reply::json(&response),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                ))
            }
        }
    }

    /// Arrête tous les sons en cours.
    ///
    /// Ne relance plus la musique de force : chaque son arrêté passe par
    /// `finish_sound`, et le dernier la relance seulement si c'est le soundboard qui
    /// l'avait mise en pause. Une musique arrêtée à la main reste arrêtée.
    pub async fn handle_soundboard_stop(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        log_info("API /api/soundboard/stop appelée");
        lock(&player).stop_all_sounds();

        let response = serde_json::json!({
            "message": "Soundboard arrêtée avec succès"
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }
}

/// Un verrou empoisonné par une panique ailleurs ne doit pas rendre le soundboard muet
/// pour de bon : l'état qu'il protège reste utilisable.
fn lock(player: &Mutex<PlayerState>) -> MutexGuard<'_, PlayerState> {
    player.lock().unwrap_or_else(|e| e.into_inner())
}

/// Client HTTP des appels à JanusCore, avec délai maximal.
fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(MUSIC_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

fn music_url(music_port: u16, action: &str) -> String {
    format!("http://127.0.0.1:{music_port}/api/{action}")
}

/// Vrai si JanusCore joue en ce moment. Injoignable ou réponse illisible vaut « ne
/// joue pas » : mieux vaut ne pas mettre en pause que relancer plus tard une musique
/// qu'on n'avait pas arrêtée.
async fn music_is_playing(music_port: u16) -> bool {
    let Ok(resp) = http().get(music_url(music_port, "status")).send().await else {
        return false;
    };
    resp.json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|json| json.get("paused").and_then(|v| v.as_bool()))
        .map(|paused| !paused)
        .unwrap_or(false)
}

/// Fichier du son demandé, confiné à `./public/soundboard`.
///
/// `sound_name` vient du réseau : `resolve_within` interdit d'en sortir (chemin absolu
/// ou `..`). Si aucune extension n'est fournie, on tente mp3, wav puis flac — chaque
/// tentative repasse par le même garde-fou.
fn resolve_sound(sound_name: &str) -> Option<PathBuf> {
    let base = Path::new("./public/soundboard");
    std::iter::once(sound_name.to_string())
        .chain(["mp3", "wav", "flac"].iter().map(|ext| format!("{sound_name}.{ext}")))
        .filter_map(|name| resolve_within(base, &name))
        .find(|path| path.is_file())
}

/// Joue un son jusqu'à sa fin ou jusqu'à un `/stop`. Bloquant.
fn play_blocking(file_path: &Path, player: &Mutex<PlayerState>) {
    let is_active_normalization = false;

    // Calculer le gain de normalisation (peut prendre un peu de temps au premier scan)
    // Normalisation désactivée pour PhonosCore :
    // on joue désormais les sons à leur volume brut (pas de EBU R128 ici).
    let normalization_gain = if is_active_normalization {
        lock(player).normalization_manager.get_or_compute_gain(file_path)
    } else {
        1.0
    };

    // Récupérer le stream_handle et le volume du player principal
    let (stream_handle, current_volume) = {
        let p = lock(player);
        (p.stream_handle.clone(), p.volume)
    };

    let source = match File::open(file_path).map(BufReader::new) {
        Ok(reader) => match Decoder::new(reader) {
            Ok(s) => s,
            Err(e) => {
                log_error(format!("Son illisible {:?} : {e}", file_path.file_name()));
                return;
            }
        },
        Err(e) => {
            log_error(format!("Ouverture impossible de {:?} : {e}", file_path.file_name()));
            return;
        }
    };
    let sink = match Sink::try_new(&stream_handle) {
        Ok(s) => s,
        Err(e) => {
            log_error(format!("Sortie audio indisponible : {e}"));
            return;
        }
    };

    // Appliquer le volume courant et le gain de normalisation
    sink.set_volume(current_volume * normalization_gain);
    sink.append(source);

    let sink = Arc::new(Mutex::new(sink));
    let (stop, stop_receiver) = mpsc::channel();
    lock(player).add_sound(ActiveSound {
        sink: sink.clone(),
        stop,
    });

    // Attendre la fin du son ou un signal d'arrêt
    loop {
        if stop_receiver.try_recv().is_ok() {
            log_info("Signal d'arrêt reçu, arrêt du son");
            if let Ok(s) = sink.lock() {
                s.stop();
            }
            break;
        }

        let is_empty = sink.lock().map(|s| s.empty()).unwrap_or(true);
        if is_empty {
            log_info("Son terminé naturellement");
            break;
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    lock(player).remove_sound(&sink);
}

/// Fin d'un son, quelle qu'en soit la cause : terminé, arrêté par `/stop` ou en échec
/// de lecture. Relance la musique à la fin du dernier son, si c'est le soundboard qui
/// l'avait mise en pause.
fn finish_sound(player: &Mutex<PlayerState>, music_port: u16) {
    if !lock(player).hold.end() {
        return;
    }

    let resumed = reqwest::blocking::Client::builder()
        .timeout(MUSIC_TIMEOUT)
        .build()
        .and_then(|client| client.get(music_url(music_port, "resume")).send());
    if let Err(e) = resumed {
        log_error(format!("Reprise de la musique impossible : {e}"));
    }
}
