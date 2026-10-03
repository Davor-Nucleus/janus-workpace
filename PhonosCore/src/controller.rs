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

use crate::model::{ActiveSound, Enqueued, MAX_WAITING, PlayerState};
use janus_log_nucleus::{log_error, log_info};
use janus_platform_nucleus::paths::resolve_within;

/// Délai maximal d'un appel à JanusCore. Injoignable ou figé, il ne doit bloquer ni
/// un son ni un handler : sans délai, reqwest attend indéfiniment.
const MUSIC_TIMEOUT: Duration = Duration::from_secs(2);

/// Délai maximal pour se connecter à JanusCore. Sous Windows, une connexion refusée
/// sur 127.0.0.1 (JanusCore arrêté) ne l'est qu'après ~2 s de nouvelles tentatives :
/// le premier son d'une série attendait d'autant avant de jouer. Lancé, JanusCore
/// accepte la connexion en moins d'une milliseconde.
const MUSIC_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);

/// Dossier des sons, relatif au dossier de lancement (la racine de PraetorCast).
const SOUNDBOARD_DIR: &str = "./public/soundboard";

/// Extensions jouées par le soundboard. Le configurateur d'alertes de
/// praetorcast-core compte les sons d'un dossier avec la même liste.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "m4a"];

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

        let reply = format!("Son {:?} joué", file_path.file_name().unwrap_or_default());
        start_sound(file_path, player, music_port).await;
        Ok(warp::reply::with_status(reply, warp::http::StatusCode::OK))
    }

    /// Joue un son tiré au hasard dans un dossier de `./public/soundboard`.
    ///
    /// `folder` est relatif à ce dossier ; absent ou vide, c'est sa racine, celle des
    /// sons de la grille de /music-config. Les sous-dossiers du dossier visé ne
    /// participent pas au tirage.
    ///
    /// Les sons tirés passent par une file : ils jouent l'un après l'autre, jamais par
    /// dessus (cf. `RandomQueue`). Répond `{ "sound": "dossier/son.mp3", "ahead": 2 }`,
    /// `ahead` étant le nombre de sons à passer avant lui (`0` : il joue tout de suite),
    /// ou `429` quand la file est pleine.
    pub async fn handle_soundboard_random(
        params: HashMap<String, String>,
        player: Arc<Mutex<PlayerState>>,
        music_port: u16,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let folder = params
            .get("folder")
            .map(|f| f.trim_matches(['/', '\\']))
            .unwrap_or("");
        let json = |body: serde_json::Value, status| {
            warp::reply::with_status(warp::reply::json(&body), status)
        };

        // Même confinement que `/play` : le dossier vient du réseau.
        let Some(dir) = resolve_within(Path::new(SOUNDBOARD_DIR), folder).filter(|p| p.is_dir())
        else {
            return Ok(json(
                serde_json::json!({ "error": format!("Dossier {folder:?} introuvable dans public/soundboard") }),
                warp::http::StatusCode::NOT_FOUND,
            ));
        };
        let files = match audio_files(&dir) {
            Ok(files) => files,
            Err(e) => {
                return Ok(json(
                    serde_json::json!({ "error": format!("Lecture du dossier {folder:?} impossible : {e}") }),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }
        };

        let picked = {
            let mut p = lock(&player);
            let key = dir.to_string_lossy().to_string();
            let last = p.last_random.get(&key).map(String::as_str);
            let picked = pick_random(&files, last, rand::random()).cloned();
            if let Some(name) = &picked {
                p.last_random.insert(key, name.clone());
            }
            picked
        };
        let Some(name) = picked else {
            return Ok(json(
                serde_json::json!({ "error": format!("Aucun son dans le dossier {folder:?}") }),
                warp::http::StatusCode::NOT_FOUND,
            ));
        };

        let sound = if folder.is_empty() { name.clone() } else { format!("{folder}/{name}") };
        let queued = lock(&player).random_queue.push(dir.join(&name));
        let ahead = match queued {
            Enqueued::Full => {
                return Ok(json(
                    serde_json::json!({ "error": format!("File de la soundboard pleine : {MAX_WAITING} sons attendent déjà") }),
                    warp::http::StatusCode::TOO_MANY_REQUESTS,
                ));
            }
            // Réponse sans attendre la mise en pause de la musique : JanusCore arrêté,
            // Windows met ~2 s à refuser la connexion, et praetorcast-core, qui attend
            // cette réponse pour une alerte, aurait conclu que PhonosCore ne répondait pas.
            Enqueued::StartWorker => {
                tokio::spawn(run_random_queue(player, music_port));
                0
            }
            Enqueued::Waiting { ahead } => ahead,
        };

        log_info(match ahead {
            0 => format!("Son tiré au hasard : {sound}"),
            _ => format!("Son tiré au hasard : {sound} (en file, {ahead} avant lui)"),
        });
        Ok(json(serde_json::json!({ "sound": sound, "ahead": ahead }), warp::http::StatusCode::OK))
    }

    /// Sous-dossiers de `./public/soundboard` et leur nombre de sons : ceux où un
    /// tirage au hasard est possible. Répond `{ "root": "C:\…\public\soundboard",
    /// "folders": [{ "name": "Memes", "sounds": 3 }] }`.
    ///
    /// `root` dit où créer un dossier : le chemin est relatif au dossier de lancement,
    /// qui n'est pas le même avec `start.bat` (racine de PraetorCast) et avec `cargo run`
    /// (dossier de PhonosCore).
    pub async fn handle_soundboard_folders() -> Result<impl Reply, std::convert::Infallible> {
        let base = Path::new(SOUNDBOARD_DIR);
        let root = std::path::absolute(base).unwrap_or_else(|_| base.to_path_buf());
        Ok(warp::reply::json(&serde_json::json!({
            "root": root.display().to_string(),
            "folders": sound_folders(base),
        })))
    }

    pub async fn handle_soundboard_sounds() -> Result<impl Reply, std::convert::Infallible> {
        let base = Path::new(SOUNDBOARD_DIR);

        if !base.is_dir() {
            let response = serde_json::json!({
                "error": "Le dossier soundboard n'existe pas"
            });
            return Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::NOT_FOUND,
            ));
        }

        match audio_files(base) {
            Ok(sounds) => {
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

    /// Arrête tous les sons en cours, et vide la file des sons tirés au hasard.
    ///
    /// Ne relance plus la musique de force : chaque son arrêté passe par
    /// `finish_sound`, et le dernier la relance seulement si c'est le soundboard qui
    /// l'avait mise en pause. Une musique arrêtée à la main reste arrêtée.
    pub async fn handle_soundboard_stop(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        log_info("API /api/soundboard/stop appelée");
        {
            // Sous le même verrou : la file vidée d'abord, sinon le son arrêté
            // laisserait aussitôt la place au suivant.
            let mut p = lock(&player);
            let dropped = p.random_queue.clear();
            if dropped > 0 {
                log_info(format!("{dropped} son(s) en attente retiré(s) de la file"));
            }
            p.stop_all_sounds();
        }

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
            .connect_timeout(MUSIC_CONNECT_TIMEOUT)
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
    let base = Path::new(SOUNDBOARD_DIR);
    std::iter::once(sound_name.to_string())
        .chain(["mp3", "wav", "flac"].iter().map(|ext| format!("{sound_name}.{ext}")))
        .filter_map(|name| resolve_within(base, &name))
        .find(|path| path.is_file())
}

/// Noms des fichiers audio de `dir`, sans descendre dans ses sous-dossiers, dans
/// l'ordre du système de fichiers : celui que la grille de /music-config a toujours
/// eu, et dont dépendent ses raccourcis alt+1…9.
fn audio_files(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut sounds = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let is_audio = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()));
        if !path.is_file() || !is_audio {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            sounds.push(name.to_string());
        }
    }
    Ok(sounds)
}

/// Sous-dossier de la soundboard, tel que le liste `/api/soundboard/folders`.
#[derive(serde::Serialize, Debug, PartialEq)]
struct SoundFolder {
    name: String,
    /// Sons jouables du dossier, sous-dossiers non compris : ceux du tirage.
    sounds: usize,
}

/// Sous-dossiers de `base`, par ordre alphabétique sans tenir compte de la casse. Un
/// `base` absent n'en a aucun.
fn sound_folders(base: &Path) -> Vec<SoundFolder> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut folders: Vec<SoundFolder> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let sounds = audio_files(&entry.path()).map(|files| files.len()).unwrap_or(0);
            Some(SoundFolder { name, sounds })
        })
        .collect();
    folders.sort_by_key(|folder| folder.name.to_lowercase());
    folders
}

/// Son à jouer parmi `files`, en écartant `last` quand il reste un autre choix : un
/// tirage qui ressort le même son deux fois de suite paraît cassé. `roll` est le
/// hasard, passé en paramètre pour les tests.
fn pick_random<'a>(files: &'a [String], last: Option<&str>, roll: usize) -> Option<&'a String> {
    let candidates: Vec<&String> = files
        .iter()
        .filter(|file| files.len() == 1 || Some(file.as_str()) != last)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some(candidates[roll % candidates.len()])
}

/// Joue la file des sons tirés au hasard jusqu'à ce qu'elle soit vide.
///
/// Une seule pause de la musique pour toute la file : elle ne reprend pas entre deux
/// sons, seulement après le dernier. La tâche tient pour cela une place dans
/// `MusicHold` du début à la fin, comme un son unique — un son de la grille joué
/// entre-temps la partage, comme deux sons qui se chevauchent.
async fn run_random_queue(player: Arc<Mutex<PlayerState>>, music_port: u16) {
    let first = lock(&player).hold.begin();
    if first && music_is_playing(music_port).await {
        let _ = http().get(music_url(music_port, "pause")).send().await;
        lock(&player).hold.set_resume(true);
    }

    loop {
        // Le verrou est relâché avant de jouer : `/stop` doit pouvoir vider la file
        // pendant qu'un son tourne.
        let next = lock(&player).random_queue.pop();
        let Some(file) = next else { break };
        let p = player.clone();
        let _ = tokio::task::spawn_blocking(move || play_blocking(&file, &p)).await;
    }

    // `finish_sound` appelle JanusCore avec le client bloquant de reqwest, interdit
    // sur un thread du runtime.
    let _ = tokio::task::spawn_blocking(move || finish_sound(&player, music_port)).await;
}

/// Lance un son du soundboard sans attendre sa fin : met la musique en pause s'il est
/// le premier d'une série, puis le joue sur un thread bloquant. Partagé par le son
/// demandé (`/play`) et le son tiré au hasard (`/random`).
async fn start_sound(file_path: PathBuf, player: Arc<Mutex<PlayerState>>, music_port: u16) {
    // Seul le premier son d'une série décide de la pause (cf. `MusicHold`).
    let first = lock(&player).hold.begin();
    if first && music_is_playing(music_port).await {
        let _ = http().get(music_url(music_port, "pause")).send().await;
        lock(&player).hold.set_resume(true);
    }

    // Joue le son du soundboard (via tokio spawn_blocking pour ne pas bloquer le runtime)
    tokio::task::spawn_blocking(move || {
        play_blocking(&file_path, &player);
        // Toujours appelé, y compris quand la lecture a échoué avant de
        // commencer : sinon le compteur de sons ne redescendrait jamais.
        finish_sound(&player, music_port);
    });
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
        .connect_timeout(MUSIC_CONNECT_TIMEOUT)
        .timeout(MUSIC_TIMEOUT)
        .build()
        .and_then(|client| client.get(music_url(music_port, "resume")).send());
    if let Err(e) = resumed {
        log_error(format!("Reprise de la musique impossible : {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn un_dossier_vide_ne_tire_rien() {
        assert_eq!(pick_random(&[], None, 0), None);
    }

    #[test]
    fn un_son_seul_ressort_meme_s_il_vient_de_jouer() {
        let files = names(&["seul.mp3"]);
        assert_eq!(pick_random(&files, Some("seul.mp3"), 7).unwrap(), "seul.mp3");
    }

    #[test]
    fn le_dernier_son_ne_ressort_pas_deux_fois_de_suite() {
        let files = names(&["a.mp3", "b.mp3", "c.mp3"]);
        for roll in 0..20 {
            assert_ne!(pick_random(&files, Some("b.mp3"), roll).unwrap(), "b.mp3", "roll {roll}");
        }
    }

    #[test]
    fn chaque_son_peut_sortir() {
        let files = names(&["a.mp3", "b.mp3", "c.mp3"]);
        let mut seen: Vec<&String> = (0..3).map(|roll| pick_random(&files, None, roll).unwrap()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn un_dernier_son_disparu_du_dossier_n_ecarte_rien() {
        let files = names(&["a.mp3", "b.mp3"]);
        let mut seen: Vec<&String> =
            (0..2).map(|roll| pick_random(&files, Some("supprime.mp3"), roll).unwrap()).collect();
        seen.sort();
        assert_eq!(seen, ["a.mp3", "b.mp3"]);
    }

    #[test]
    fn seuls_les_fichiers_audio_du_dossier_sont_tires() {
        // Dossier propre au test : jamais `public/soundboard`, qui porte les vrais sons.
        let dir = std::env::temp_dir().join(format!("phonoscore-random-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sous-dossier")).unwrap();
        for file in ["rire.mp3", "CRI.WAV", "notes.txt", "sous-dossier/cache.mp3"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }

        let mut files = audio_files(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        files.sort();
        assert_eq!(files, ["CRI.WAV", "rire.mp3"]);
    }

    #[test]
    fn les_sous_dossiers_sont_listes_avec_leurs_sons() {
        let base = std::env::temp_dir().join(format!("phonoscore-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for dir in ["Memes", "bruitages", "vide", "Memes/archives"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        for file in [
            "racine.mp3",
            "Memes/a.mp3",
            "Memes/b.OGG",
            "Memes/lisez-moi.txt",
            "Memes/archives/vieux.mp3",
            "bruitages/porte.wav",
        ] {
            std::fs::write(base.join(file), b"x").unwrap();
        }

        let found = sound_folders(&base);
        let _ = std::fs::remove_dir_all(&base);

        let folder = |name: &str, sounds| SoundFolder { name: name.to_string(), sounds };
        assert_eq!(
            found,
            vec![
                // Sans tenir compte de la casse : « bruitages » avant « Memes ».
                folder("bruitages", 1),
                // Ni le .txt ni le sous-dossier ne comptent : ils ne sortiraient pas.
                folder("Memes", 2),
                folder("vide", 0),
            ]
        );
    }

    #[test]
    fn sans_dossier_soundboard_il_n_y_a_aucun_sous_dossier() {
        assert!(sound_folders(Path::new("n-existe-pas/soundboard")).is_empty());
    }
}
