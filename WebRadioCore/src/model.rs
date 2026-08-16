//! État de la radio : playlist, piste courante, réglages.
//!
//! Volontairement dépourvu de `Sink` et de `OutputStreamHandle`, contrairement au
//! `PlayerState` de JanusCore : WebRadioCore n'ouvre aucun périphérique audio, ce
//! qui lui permet de tourner sur une machine qui n'en a pas.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use janus_nucleus::audio::NormalizationManager;
use janus_nucleus::config::update_config_key;
use janus_nucleus::logger::{log_error, log_info};
use janus_nucleus::music::{collect_tracks, list_folders};

pub use janus_nucleus::music::MUSIC_ROOT;

/// L'historique sert au diagnostic, pas à la navigation : le borner évite qu'une
/// radio laissée allumée plusieurs jours n'accumule indéfiniment des chemins.
const HISTORY_MAX: usize = 50;

pub struct RadioState {
    queue: VecDeque<PathBuf>,
    history: VecDeque<PathBuf>,
    current_file: Option<PathBuf>,
    /// Dossier de la playlist en cours, mémorisé pour le rebouclage.
    ///
    /// JanusCore reconstruit sa playlist depuis `current_file.parent()`
    /// (`model.rs:181`). Comme le parcours est récursif, une piste rangée dans un
    /// sous-dossier y réduit la playlist à ce seul sous-dossier au moment de
    /// reboucler. Garder le dossier demandé évite ce rétrécissement.
    current_folder: Option<PathBuf>,
    volume: f32,
    paused: bool,
    normalization_enabled: bool,
    normalization_manager: Arc<NormalizationManager>,
    /// Signal « le moteur doit rouvrir une piste ».
    ///
    /// Incrémenté aussi bien par un changement de playlist que par un saut avant
    /// ou arrière : dans les trois cas le moteur doit abandonner la piste en cours
    /// et dépiler la suivante, donc un seul compteur suffit. Un compteur plutôt
    /// qu'un booléen « il faut redémarrer », qu'il faudrait remettre à zéro — et
    /// dont la remise à zéro ouvrirait une course entre les deux fils.
    generation: u64,
}

impl RadioState {
    pub fn new(volume: f32, normalization_enabled: bool) -> Self {
        Self {
            queue: VecDeque::new(),
            history: VecDeque::new(),
            current_file: None,
            current_folder: None,
            volume: volume.clamp(0.0, 1.0),
            paused: false,
            normalization_enabled,
            normalization_manager: Arc::new(NormalizationManager::default()),
            generation: 0,
        }
    }

    /// Remplace la playlist par le contenu mélangé de `folder`.
    ///
    /// Renvoie le nombre de pistes retenues ; 0 laisse l'état inchangé.
    pub fn load_folder(&mut self, folder: &Path) -> usize {
        let files = collect_tracks(folder);
        if files.is_empty() {
            log_info(format!("Aucun fichier audio trouvé dans {folder:?}"));
            return 0;
        }

        let count = files.len();
        self.queue = VecDeque::from(files);
        self.current_folder = Some(folder.to_path_buf());
        self.generation = self.generation.wrapping_add(1);
        count
    }

    /// Dépile la piste suivante, en rebouclant sur le dossier courant si besoin.
    pub fn next_track(&mut self) -> Option<PathBuf> {
        if self.queue.is_empty() {
            let folder = self.current_folder.clone()?;
            let files = collect_tracks(&folder);
            if files.is_empty() {
                return None;
            }
            log_info("Fin de playlist — rebouclage".to_string());
            self.queue = VecDeque::from(files);
        }

        let next = self.queue.pop_front()?;
        if let Some(previous) = self.current_file.replace(next.clone()) {
            self.history.push_back(previous);
            if self.history.len() > HISTORY_MAX {
                self.history.pop_front();
            }
        }
        Some(next)
    }

    /// Prochaine piste sans la dépiler, pour préchauffer son analyse de loudness.
    pub fn peek_next(&self) -> Option<&PathBuf> {
        self.queue.front()
    }

    /// Demande au moteur d'abandonner la piste en cours et de dépiler la suivante.
    ///
    /// L'historique est géré par [`Self::next_track`], que le moteur appellera : il
    /// n'y a rien d'autre à faire ici que signaler.
    pub fn skip_next(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Replace la piste précédente en tête de file. `false` si l'historique est vide.
    pub fn play_previous(&mut self) -> bool {
        let Some(previous) = self.history.pop_back() else {
            return false;
        };

        // `take()` et non `clone()` : `next_track` empile dans l'historique ce que
        // `current_file.replace(...)` lui rend. En vidant le champ, on garantit
        // qu'un retour arrière n'empile rien — sinon `previous` deux fois de suite
        // ferait du sur-place entre les deux mêmes pistes.
        if let Some(current) = self.current_file.take() {
            self.queue.push_front(current);
        }
        self.queue.push_front(previous);
        self.generation = self.generation.wrapping_add(1);
        true
    }

    pub fn has_previous(&self) -> bool {
        !self.history.is_empty()
    }

    /// Y a-t-il une suite ?
    ///
    /// Vrai dès qu'un dossier est chargé, même file vide : la radio reboucle
    /// indéfiniment, donc il y a toujours une piste après.
    pub fn has_next(&self) -> bool {
        !self.queue.is_empty() || self.current_folder.is_some()
    }

    pub fn previous_music_name(&self) -> Option<String> {
        self.history
            .back()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Chemin de la piste en cours, à sonder **après** avoir relâché le verrou.
    ///
    /// Lire les tags coûte plusieurs millisecondes ; le faire verrou en main
    /// bloquerait le moteur, qui le reprend toutes les 192 ms, et se traduirait par
    /// un blanc pour tous les auditeurs.
    pub fn current_path(&self) -> Option<PathBuf> {
        self.current_file.clone()
    }

    /// Règle le volume et le persiste dans `env.json`.
    ///
    /// Clé `WEBRADIO_VOLUME`, distincte de `VOLUME` que se partagent déjà JanusCore
    /// et PhonosCore : la radio est seule à l'écrire, donc aucun conflit d'écrivains.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Err(e) = update_config_key("WEBRADIO_VOLUME", serde_json::json!(self.volume)) {
            log_error(format!("Volume non persisté dans env.json : {e}"));
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn normalization_enabled(&self) -> bool {
        self.normalization_enabled
    }

    pub fn normalization_manager(&self) -> Arc<NormalizationManager> {
        Arc::clone(&self.normalization_manager)
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn current_music_name(&self) -> Option<String> {
        self.current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
    }
}

/// Dossiers de playlist disponibles sous [`MUSIC_ROOT`].
pub fn get_folders_list() -> Vec<String> {
    list_folders(MUSIC_ROOT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(nom: &str, pistes: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join("webradiocore_test").join(nom);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sous-dossier")).unwrap();
        for piste in pistes {
            fs::write(dir.join(piste), b"factice").unwrap();
        }
        dir
    }

    #[test]
    fn ne_retient_que_les_extensions_audio() {
        let dir = fixture("extensions", &["a.mp3", "b.flac", "c.wav", "notes.txt"]);
        let mut state = RadioState::new(1.0, false);
        assert_eq!(state.load_folder(&dir), 3);
    }

    #[test]
    fn un_dossier_vide_laisse_la_playlist_intacte() {
        let plein = fixture("plein", &["a.mp3"]);
        let vide = fixture("vide", &[]);

        let mut state = RadioState::new(1.0, false);
        state.load_folder(&plein);
        let generation = state.generation();

        assert_eq!(state.load_folder(&vide), 0);
        assert_eq!(state.queue_len(), 1, "la playlist a été écrasée");
        assert_eq!(state.generation(), generation, "génération incrémentée à tort");
    }

    #[test]
    fn changer_de_playlist_incremente_la_generation() {
        let dir = fixture("generation", &["a.mp3"]);
        let mut state = RadioState::new(1.0, false);

        let avant = state.generation();
        state.load_folder(&dir);
        assert_ne!(state.generation(), avant);
    }

    /// Le rebouclage doit repartir du dossier demandé, et non du dossier parent de
    /// la dernière piste jouée — sinon une piste rangée dans un sous-dossier y
    /// enfermerait la radio.
    #[test]
    fn reboucle_sur_le_dossier_demande_pas_sur_le_sous_dossier() {
        let dir = fixture("reboucle", &["a.mp3"]);
        fs::write(dir.join("sous-dossier").join("b.mp3"), b"factice").unwrap();

        let mut state = RadioState::new(1.0, false);
        assert_eq!(state.load_folder(&dir), 2);

        // Épuise la playlist, puis force un tour de plus.
        state.next_track().unwrap();
        state.next_track().unwrap();
        assert_eq!(state.queue_len(), 0);

        state.next_track().unwrap();
        // Le rebouclage doit avoir retrouvé les deux pistes, pas seulement celles
        // du dossier de la dernière piste jouée.
        assert_eq!(state.queue_len(), 1);
    }

    #[test]
    fn l_historique_reste_borne() {
        let dir = fixture("historique", &["a.mp3"]);
        let mut state = RadioState::new(1.0, false);
        state.load_folder(&dir);

        for _ in 0..(HISTORY_MAX * 2 + 10) {
            state.next_track().unwrap();
        }
        assert!(state.history.len() <= HISTORY_MAX);
    }

    /// Sans playlist chargée, rien à jouer : le moteur diffusera du silence plutôt
    /// que de fermer le flux.
    #[test]
    fn sans_playlist_il_n_y_a_pas_de_piste_suivante() {
        let mut state = RadioState::new(1.0, false);
        assert!(state.next_track().is_none());
        assert!(state.current_music_name().is_none());
    }

    #[test]
    fn le_volume_est_borne_a_la_construction() {
        assert_eq!(RadioState::new(5.0, false).volume(), 1.0);
        assert_eq!(RadioState::new(-1.0, false).volume(), 0.0);
    }

    #[test]
    fn play_previous_sans_historique_ne_fait_rien() {
        let dir = fixture("prev_vide", &["a.mp3"]);
        let mut state = RadioState::new(1.0, false);
        state.load_folder(&dir);

        let generation = state.generation();
        assert!(!state.has_previous());
        assert!(!state.play_previous());
        assert_eq!(
            state.generation(),
            generation,
            "génération incrémentée alors que rien n'a bougé"
        );
    }

    /// Le comportement qui décide de la justesse de la navigation : après un
    /// aller-retour, on doit retomber sur la piste d'où l'on vient.
    #[test]
    fn next_puis_previous_revient_sur_la_meme_piste() {
        let dir = fixture("aller_retour", &["a.mp3", "b.mp3", "c.mp3"]);
        let mut state = RadioState::new(1.0, false);
        state.load_folder(&dir);

        let premiere = state.next_track().unwrap();
        let deuxieme = state.next_track().unwrap();
        assert_ne!(premiere, deuxieme);
        assert!(state.has_previous());

        assert!(state.play_previous());
        // Le moteur dépile ensuite : il doit retrouver la première piste.
        assert_eq!(state.next_track().unwrap(), premiere);
    }

    /// Reculer deux fois doit reculer de deux pistes. Si `play_previous` clonait la
    /// piste courante au lieu de la retirer, `next_track` la réempilerait dans
    /// l'historique et l'on ferait du sur-place entre deux morceaux.
    #[test]
    fn deux_previous_consecutifs_reculent_bien_de_deux() {
        let dir = fixture("recul_double", &["a.mp3", "b.mp3", "c.mp3"]);
        let mut state = RadioState::new(1.0, false);
        state.load_folder(&dir);

        let p1 = state.next_track().unwrap();
        let _p2 = state.next_track().unwrap();
        let p3 = state.next_track().unwrap();
        assert_eq!(state.current_path().unwrap(), p3);

        // Recule sur p2.
        assert!(state.play_previous());
        let revenu2 = state.next_track().unwrap();
        // Recule sur p1.
        assert!(state.play_previous());
        let revenu1 = state.next_track().unwrap();

        assert_ne!(revenu2, revenu1, "sur-place entre deux pistes");
        assert_eq!(revenu1, p1);
    }

    #[test]
    fn has_next_est_vrai_des_qu_un_dossier_est_charge() {
        let dir = fixture("has_next", &["a.mp3"]);
        let mut state = RadioState::new(1.0, false);
        assert!(!state.has_next(), "aucun dossier chargé");

        state.load_folder(&dir);
        assert!(state.has_next());

        // File vidée, mais la radio reboucle : il y a toujours une suite.
        state.next_track().unwrap();
        assert_eq!(state.queue_len(), 0);
        assert!(state.has_next());
    }

    #[test]
    fn skip_next_signale_le_moteur_sans_toucher_a_la_file() {
        let dir = fixture("skip", &["a.mp3", "b.mp3"]);
        let mut state = RadioState::new(1.0, false);
        state.load_folder(&dir);

        let generation = state.generation();
        let file_avant = state.queue_len();
        state.skip_next();

        assert_ne!(state.generation(), generation);
        assert_eq!(state.queue_len(), file_avant, "la file ne doit pas bouger");
    }

    #[test]
    fn la_pause_se_memorise() {
        let mut state = RadioState::new(1.0, false);
        assert!(!state.is_paused());
        state.set_paused(true);
        assert!(state.is_paused());
        state.set_paused(false);
        assert!(!state.is_paused());
    }
}
