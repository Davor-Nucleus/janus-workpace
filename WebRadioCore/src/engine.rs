//! Producteur du flux : décodage → rééchantillonnage → gain → MP3 → diffusion.

use rodio::source::UniformSourceIterator;
use rodio::Decoder;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use janus_nucleus::audio::NormalizationManager;
use janus_nucleus::logger::{log_error, log_info};
use janus_nucleus::stream::{
    spawn_encode_loop, StreamHub, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE,
};

use crate::model::RadioState;

/// Nombre de pistes illisibles tolérées avant de rendre la main.
///
/// Remplace la récursion de `JanusCore::play_next` (`model.rs:163,173`), qui
/// s'appelle elle-même à chaque fichier illisible et finit par déborder la pile
/// sur un dossier entièrement corrompu.
const MAX_OPEN_ATTEMPTS: usize = 8;

/// Piste en cours de diffusion.
struct Track {
    samples: Box<dyn Iterator<Item = i16> + Send>,
    /// Gain de normalisation, appliqué en plus du volume.
    gain: f32,
}

/// Démarre le moteur : la boucle est celle de `janus_nucleus`, seul le
/// remplissage des blocs est propre à la webradio.
pub fn spawn(
    state: Arc<Mutex<RadioState>>,
    hub: Arc<StreamHub>,
    bitrate_kbps: u16,
    lead: Duration,
    shutdown: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let mut track: Option<Track> = None;
    let mut generation = u64::MAX; // force une resynchronisation au premier tour

    spawn_encode_loop(hub, bitrate_kbps, lead, shutdown, "webradio", move |pcm| {
        let (current_generation, volume, paused) = {
            let s = state.lock().unwrap();
            (s.generation(), s.volume(), s.is_paused())
        };

        // Playlist changée, ou saut demandé : on abandonne la piste en cours.
        if current_generation != generation {
            generation = current_generation;
            track = None;
        }

        // En pause, on ne tire rien de la piste : elle n'est pas consommée, donc la
        // reprise repart exactement où elle s'était arrêtée. La boucle continue de
        // cadencer et le hub reçoit du silence encodé, si bien que le flux ne se
        // ferme pas et qu'aucun auditeur n'est déconnecté.
        let filled = if paused {
            0
        } else {
            fill_chunk(pcm, &mut track, &state, volume)
        };
        // Rien à jouer, ou piste plus courte que le bloc : on complète en silence
        // plutôt que de s'arrêter. Un auditeur peut ainsi se connecter avant toute
        // playlist et entendre la musique démarrer, sans se reconnecter.
        pcm[filled..].fill(0);
    })
}

/// Remplit `pcm` avec les échantillons disponibles ; renvoie la quantité écrite.
///
/// Enchaîne les pistes **à l'intérieur** d'un même bloc : sans cela, chaque
/// changement de piste insérerait jusqu'à un bloc entier de silence, soit un blanc
/// audible à chaque morceau.
fn fill_chunk(
    pcm: &mut [i16],
    track: &mut Option<Track>,
    state: &Arc<Mutex<RadioState>>,
    volume: f32,
) -> usize {
    let mut filled = 0;
    let mut attempts = 0;

    while filled < pcm.len() {
        if track.is_none() {
            // Garde-fou distinct de celui d'`open_next_track` : couvre le cas d'un
            // fichier qui s'ouvre correctement mais ne produit aucun échantillon.
            attempts += 1;
            if attempts > MAX_OPEN_ATTEMPTS {
                break;
            }
            match open_next_track(state) {
                Some(t) => *track = Some(t),
                None => break, // plus rien à jouer : le reste du bloc sera du silence
            }
        }

        let current = track.as_mut().expect("piste ouverte juste au-dessus");
        // Recalculé à chaque bloc, donc régler le volume en cours de piste ne perd
        // pas le gain de normalisation — le défaut de `JanusCore::set_volume`
        // (`model.rs:199-203`), qui écrase le produit posé à `model.rs:147`.
        let gain = volume * current.gain;

        while filled < pcm.len() {
            match current.samples.next() {
                Some(sample) => {
                    pcm[filled] = scale(sample, gain);
                    filled += 1;
                }
                None => break,
            }
        }

        if filled < pcm.len() {
            *track = None; // piste épuisée : on enchaîne dans le même bloc
        }
    }

    filled
}

#[inline]
fn scale(sample: i16, gain: f32) -> i16 {
    if (gain - 1.0).abs() < f32::EPSILON {
        return sample;
    }
    // La normalisation peut amplifier jusqu'à 5,6× : sans saturation explicite, la
    // conversion déborderait et replierait le signal en craquements.
    (sample as f32 * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Ouvre la piste suivante, en sautant celles qui échouent.
fn open_next_track(state: &Arc<Mutex<RadioState>>) -> Option<Track> {
    for _ in 0..MAX_OPEN_ATTEMPTS {
        let path = state.lock().unwrap().next_track()?;
        if let Some(track) = open_track(&path, state) {
            return Some(track);
        }
    }
    log_error(format!(
        "{MAX_OPEN_ATTEMPTS} pistes illisibles d'affilée — diffusion de silence"
    ));
    None
}

fn open_track(path: &Path, state: &Arc<Mutex<RadioState>>) -> Option<Track> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            log_error(format!("Ouverture impossible de {path:?} : {e}"));
            return None;
        }
    };

    let decoder = match Decoder::new(BufReader::new(file)) {
        Ok(d) => d,
        Err(e) => {
            log_error(format!("Décodage impossible de {path:?} : {e}"));
            return None;
        }
    };

    // Ramène tout au format unique de l'encodeur : LAME est verrouillé sur une
    // fréquence et un nombre de canaux, alors que la bibliothèque mélange 44,1 et
    // 48 kHz, mono et stéréo.
    let samples =
        UniformSourceIterator::<_, i16>::new(decoder, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE);

    let gain = resolve_gain(path, state);
    log_info(format!(
        "Diffusion : {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    Some(Track {
        samples: Box::new(samples),
        gain,
    })
}

/// Gain de normalisation, **sans jamais bloquer**.
///
/// `get_or_compute_gain` décode le fichier entier quand le cache est froid
/// (`janus_nucleus/src/audio.rs:72`). L'appeler ici figerait le producteur
/// plusieurs secondes, donc tous les auditeurs. On se contente du cache, et on
/// lance l'analyse en tâche de fond : la piste courante passe sans gain cette
/// fois-ci, la suivante est prête à temps, et les écoutes ultérieures sont
/// normalisées.
fn resolve_gain(path: &Path, state: &Arc<Mutex<RadioState>>) -> f32 {
    let (enabled, manager, next) = {
        let s = state.lock().unwrap();
        (
            s.normalization_enabled(),
            s.normalization_manager(),
            s.peek_next().cloned(),
        )
    };

    if !enabled {
        return 1.0;
    }

    let gain = match manager.cached_gain(path) {
        Some(g) => g,
        None => {
            warm_gain(&manager, path.to_path_buf());
            1.0
        }
    };

    // Préchauffe la piste suivante pour que la transition, elle, soit normalisée.
    if let Some(next) = next {
        if manager.cached_gain(&next).is_none() {
            warm_gain(&manager, next);
        }
    }

    gain
}

/// Calcule un gain hors du chemin temps réel.
///
/// Le test `cached_gain().is_none()` en amont limite naturellement les doublons :
/// une fois le cache chaud, plus aucun fil n'est lancé pour ce fichier.
fn warm_gain(manager: &Arc<NormalizationManager>, path: PathBuf) {
    let manager = Arc::clone(manager);
    let spawned = thread::Builder::new()
        .name("webradio-loudness".to_string())
        .spawn(move || {
            manager.get_or_compute_gain(&path);
        });

    if let Err(e) = spawned {
        log_error(format!("Analyse de loudness non lancée : {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piste(samples: Vec<i16>, gain: f32) -> Track {
        Track {
            samples: Box::new(samples.into_iter()),
            gain,
        }
    }

    #[test]
    fn sature_au_lieu_de_replier() {
        // 20 000 × 5,6 dépasse largement i16 : sans saturation, la valeur se
        // replierait en négatif et s'entendrait comme un craquement.
        assert_eq!(scale(20_000, 5.6), i16::MAX);
        assert_eq!(scale(-20_000, 5.6), i16::MIN);
    }

    #[test]
    fn laisse_le_signal_intact_a_gain_unitaire() {
        assert_eq!(scale(1234, 1.0), 1234);
        assert_eq!(scale(i16::MIN, 1.0), i16::MIN);
    }

    #[test]
    fn applique_le_produit_volume_gain() {
        assert_eq!(scale(1000, 0.5), 500);
        // Volume 0,5 et gain de normalisation 2,0 se compensent.
        assert_eq!(scale(1000, 0.5 * 2.0), 1000);
    }

    /// Sans playlist, le bloc doit rester vide — l'appelant le complète en silence
    /// plutôt que d'arrêter le flux.
    #[test]
    fn sans_piste_le_bloc_reste_vide() {
        let state = Arc::new(Mutex::new(RadioState::new(1.0, false)));
        let mut pcm = vec![7i16; 64];
        let mut track = None;

        assert_eq!(fill_chunk(&mut pcm, &mut track, &state, 1.0), 0);
    }

    /// Le cœur du rendu sans blanc : deux pistes courtes doivent se suivre à
    /// l'intérieur d'un seul bloc.
    #[test]
    fn enchaine_deux_pistes_dans_un_meme_bloc() {
        let state = Arc::new(Mutex::new(RadioState::new(1.0, false)));
        let mut pcm = vec![0i16; 8];
        let mut track = Some(piste(vec![100, 100, 100], 1.0));

        // La première piste ne fournit que 3 échantillons ; sans playlist derrière,
        // le remplissage s'arrête là.
        let filled = fill_chunk(&mut pcm, &mut track, &state, 1.0);
        assert_eq!(filled, 3);
        assert_eq!(&pcm[..3], &[100, 100, 100]);
        assert!(track.is_none(), "la piste épuisée aurait dû être libérée");
    }

    #[test]
    fn le_volume_s_applique_au_remplissage() {
        let state = Arc::new(Mutex::new(RadioState::new(1.0, false)));
        let mut pcm = vec![0i16; 4];
        let mut track = Some(piste(vec![1000, 1000, 1000, 1000], 1.0));

        fill_chunk(&mut pcm, &mut track, &state, 0.5);
        assert_eq!(pcm, vec![500, 500, 500, 500]);
    }

    /// La propriété qui fait tout l'intérêt du gel de piste : en pause, la piste
    /// n'est pas consommée, donc la reprise reprend au même échantillon.
    #[test]
    fn la_pause_ne_consomme_pas_la_piste() {
        let state = Arc::new(Mutex::new(RadioState::new(1.0, false)));
        state.lock().unwrap().set_paused(true);

        let mut pcm = vec![0i16; 4];
        let mut track = Some(piste(vec![11, 22, 33, 44, 55, 66], 1.0));

        // Deux tours de la boucle du moteur, en pause.
        for _ in 0..2 {
            let paused = state.lock().unwrap().is_paused();
            let filled = if paused {
                0
            } else {
                fill_chunk(&mut pcm, &mut track, &state, 1.0)
            };
            pcm[filled..].fill(0);
            assert_eq!(filled, 0);
            assert_eq!(pcm, vec![0, 0, 0, 0], "du son est sorti pendant la pause");
        }

        // À la reprise, la piste doit rendre son tout premier échantillon.
        state.lock().unwrap().set_paused(false);
        let filled = fill_chunk(&mut pcm, &mut track, &state, 1.0);
        assert_eq!(filled, 4);
        assert_eq!(pcm, vec![11, 22, 33, 44], "la piste a avancé pendant la pause");
    }

    /// Une piste qui s'ouvre sans jamais produire d'échantillon ne doit pas faire
    /// tourner le remplissage en boucle.
    #[test]
    fn une_piste_vide_ne_boucle_pas_indefiniment() {
        let state = Arc::new(Mutex::new(RadioState::new(1.0, false)));
        let mut pcm = vec![0i16; 16];
        let mut track = Some(piste(vec![], 1.0));

        assert_eq!(fill_chunk(&mut pcm, &mut track, &state, 1.0), 0);
    }
}
