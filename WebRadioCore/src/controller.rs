//! Handlers Warp de WebRadioCore.

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;
use warp::ws::{Message, Ws};
use warp::{Rejection, Reply};

use janus_nucleus::logger::log_info;
use janus_nucleus::metadata::read_metadata;
use janus_nucleus::paths::resolve_within;
use janus_nucleus::stream::StreamHub;

use crate::model::{get_folders_list, RadioState, MUSIC_ROOT};

/// Corps JSON de `POST /api/volume`.
#[derive(Deserialize)]
pub struct VolumeRequest {
    pub volume: f32,
}

/// Dépendances partagées par les handlers.
pub struct RadioContext {
    pub state: Arc<Mutex<RadioState>>,
    pub hub: Arc<StreamHub>,
    pub bitrate_kbps: u16,
}

/// Métadonnées de la piste en cours, **sondées hors verrou**.
///
/// C'est la règle qui structure tout ce fichier : le moteur reprend le verrou
/// d'état toutes les 192 ms, et lire les tags coûte plusieurs millisecondes. Sonder
/// verrou en main le ferait attendre, et le blanc s'entendrait chez tous les
/// auditeurs à la fois. On prend donc le verrou juste le temps de copier le chemin.
fn current_metadata(state: &Mutex<RadioState>) -> Option<serde_json::Value> {
    let path: Option<PathBuf> = state.lock().unwrap().current_path();
    let meta = read_metadata(&path?)?;
    serde_json::to_value(&meta).ok()
}

pub struct RadioController;

impl RadioController {
    /// Le flux lui-même.
    ///
    /// Aucun `Content-Length` : hyper bascule alors en `Transfer-Encoding:
    /// chunked`, ce qu'attend un flux sans fin. Les navigateurs envoient parfois
    /// `Range: bytes=0-` sur une balise `<audio>` ; on l'ignore et on répond 200,
    /// d'où l'`Accept-Ranges: none` qui le leur annonce.
    pub async fn handle_stream(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        // L'abonnement porte le comptage des auditeurs : il vit exactement aussi
        // longtemps que le corps de la réponse, y compris si le client coupe net.
        let chunks = ctx.hub.subscribe().map(Ok::<Bytes, Infallible>);

        let response = Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "audio/mpeg")
            .header("Cache-Control", "no-store, no-cache, must-revalidate")
            .header("Pragma", "no-cache")
            .header("Accept-Ranges", "none")
            .header("icy-name", "WebRadioCore")
            .body(Body::wrap_stream(chunks))
            .expect("réponse de flux mal formée");

        Ok(response)
    }

    /// État courant de la radio.
    pub async fn handle_status(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        let (current_music, volume, queue_len, paused, has_next, has_previous) = {
            let s = ctx.state.lock().unwrap();
            (
                s.current_music_name(),
                s.volume(),
                s.queue_len(),
                s.is_paused(),
                s.has_next(),
                s.has_previous(),
            )
        };

        let payload = serde_json::json!({
            "playing": current_music.is_some() && !paused,
            "paused": paused,
            "current_music": current_music,
            "volume": volume,
            "queue_len": queue_len,
            "has_next": has_next,
            "has_previous": has_previous,
            "listeners": ctx.hub.listener_count(),
            "bitrate_kbps": ctx.bitrate_kbps,
            "stream_url": "/stream.mp3",
        });

        Ok(warp::reply::json(&payload))
    }

    /// Métadonnées de la piste en cours (titre, artiste, album, date, pochette).
    pub async fn handle_current_music(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        match current_metadata(&ctx.state) {
            Some(meta) => Ok(warp::reply::json(&meta)),
            None => Ok(warp::reply::json(
                &serde_json::json!({ "message": "Aucune musique en cours de lecture." }),
            )),
        }
    }

    /// Passe à la piste suivante.
    pub async fn handle_next(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        let has_next = {
            let mut s = ctx.state.lock().unwrap();
            let has_next = s.has_next();
            if has_next {
                s.skip_next();
            }
            has_next
        };

        if !has_next {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Aucune playlist chargée" })),
                StatusCode::BAD_REQUEST,
            ));
        }

        // La piste effective n'est connue qu'une fois le moteur passé au bloc
        // suivant : on accuse réception, l'appelant lira /api/status ou le WebSocket.
        Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "message": "Piste suivante" })),
            StatusCode::OK,
        ))
    }

    /// Revient à la piste précédente.
    pub async fn handle_previous(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        let (ok, name) = {
            let mut s = ctx.state.lock().unwrap();
            let name = s.previous_music_name();
            (s.play_previous(), name)
        };

        if !ok {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Aucune piste précédente" })),
                StatusCode::BAD_REQUEST,
            ));
        }

        Ok(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({ "message": "Piste précédente", "current_music": name }),
            ),
            StatusCode::OK,
        ))
    }

    pub async fn handle_has_next(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        let has_next = ctx.state.lock().unwrap().has_next();
        Ok(warp::reply::json(&has_next))
    }

    pub async fn handle_has_previous(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        let has_previous = ctx.state.lock().unwrap().has_previous();
        Ok(warp::reply::json(&has_previous))
    }

    /// Gèle la piste. Le flux continue en silence, les auditeurs restent connectés.
    pub async fn handle_pause(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        ctx.state.lock().unwrap().set_paused(true);
        log_info("Pause — le flux passe au silence".to_string());
        Ok(warp::reply::json(
            &serde_json::json!({ "message": "Lecture mise en pause", "paused": true }),
        ))
    }

    /// Reprend exactement où la piste s'était arrêtée.
    pub async fn handle_resume(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        ctx.state.lock().unwrap().set_paused(false);
        log_info("Reprise de la lecture".to_string());
        Ok(warp::reply::json(
            &serde_json::json!({ "message": "Lecture reprise", "paused": false }),
        ))
    }

    pub async fn handle_get_volume(ctx: Arc<RadioContext>) -> Result<impl Reply, Infallible> {
        let volume = ctx.state.lock().unwrap().volume();
        Ok(warp::reply::json(&serde_json::json!({ "volume": volume })))
    }

    /// Règle le volume. Le moteur relit la valeur à chaque bloc, donc le changement
    /// s'entend immédiatement **sans** perdre le gain de normalisation.
    pub async fn handle_set_volume(
        req: VolumeRequest,
        ctx: Arc<RadioContext>,
    ) -> Result<impl Reply, Infallible> {
        if !req.volume.is_finite() {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Volume invalide" })),
                StatusCode::BAD_REQUEST,
            ));
        }

        let volume = {
            let mut s = ctx.state.lock().unwrap();
            s.set_volume(req.volume);
            s.volume()
        };

        Ok(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({ "message": "Volume mis à jour", "volume": volume }),
            ),
            StatusCode::OK,
        ))
    }

    /// Flux d'état temps réel pour une page « à l'écoute » ou un overlay.
    pub async fn handle_current_music_ws(
        ws: Ws,
        ctx: Arc<RadioContext>,
    ) -> Result<impl Reply, Rejection> {
        Ok(ws.on_upgrade(move |socket| async move {
            let (mut tx, mut rx) = socket.split();
            let mut last_sent: Option<String> = None;
            let mut cached_path: Option<PathBuf> = None;
            let mut cached_meta = serde_json::Value::Null;

            loop {
                // Deux prises de verrou courtes, jamais pendant la lecture du
                // fichier : la sonde symphonia se fait entre les deux.
                let path = ctx.state.lock().unwrap().current_path();
                if path != cached_path {
                    cached_meta = path
                        .as_deref()
                        .and_then(read_metadata)
                        .and_then(|m| serde_json::to_value(&m).ok())
                        .unwrap_or(serde_json::Value::Null);
                    cached_path = path;
                }

                let snapshot = {
                    let s = ctx.state.lock().unwrap();
                    serde_json::json!({
                        "queue_len": s.queue_len(),
                        "paused": s.is_paused(),
                        "volume": s.volume(),
                        "current_music": s.current_music_name(),
                        "has_next": s.has_next(),
                        "has_previous": s.has_previous(),
                        "history_len": s.history_len(),
                        "listeners": ctx.hub.listener_count(),
                        "metadata": cached_meta,
                    })
                    .to_string()
                };

                if last_sent.as_deref() != Some(snapshot.as_str()) {
                    if tx.send(Message::text(snapshot.clone())).await.is_err() {
                        break;
                    }
                    last_sent = Some(snapshot);
                }

                // Attendre le tick *et* surveiller le client en même temps.
                //
                // Draîner les messages entrants dans une tâche séparée ne suffirait
                // pas : la boucle ne remarquerait le départ du client qu'au premier
                // envoi en échec, or elle n'envoie que lorsque l'état change. Un
                // client déconnecté pendant une plage stable laisserait donc une
                // tâche tourner indéfiniment.
                tokio::select! {
                    incoming = rx.next() => match incoming {
                        // Flux clos, erreur de transport, ou trame de fermeture.
                        None | Some(Err(_)) => break,
                        Some(Ok(msg)) if msg.is_close() => break,
                        // Rien à faire d'un message client : warp répond seul aux ping.
                        Some(Ok(_)) => {}
                    },
                    _ = sleep(Duration::from_millis(500)) => {}
                }
            }
        }))
    }

    /// Charge une playlist et lance sa diffusion.
    pub async fn handle_folder(
        params: HashMap<String, String>,
        ctx: Arc<RadioContext>,
    ) -> Result<impl Reply, Infallible> {
        let Some(folder_name) = params.get("folder") else {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Paramètre 'folder' manquant" })),
                StatusCode::BAD_REQUEST,
            ));
        };

        // Le paramètre vient du réseau : il doit rester sous MUSIC_ROOT. Réponse
        // volontairement identique pour « hors base » et « inexistant », pour ne
        // rien révéler de l'arborescence du disque.
        let Some(folder_path) =
            resolve_within(Path::new(MUSIC_ROOT), folder_name).filter(|p| p.is_dir())
        else {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Dossier introuvable" })),
                StatusCode::BAD_REQUEST,
            ));
        };

        let tracks = ctx.state.lock().unwrap().load_folder(&folder_path);
        if tracks == 0 {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Aucun fichier audio jouable" })),
                StatusCode::BAD_REQUEST,
            ));
        }

        log_info(format!("Playlist chargée : {folder_name} ({tracks} pistes)"));
        Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "folder": folder_name, "tracks": tracks })),
            StatusCode::OK,
        ))
    }

    /// Playlists disponibles.
    ///
    /// Hors du strict minimum, mais sans elle rien ne permet de découvrir une
    /// valeur valide pour `folder`.
    pub async fn handle_folderlist() -> Result<impl Reply, Infallible> {
        Ok(warp::reply::json(
            &serde_json::json!({ "folders": get_folders_list() }),
        ))
    }
}
