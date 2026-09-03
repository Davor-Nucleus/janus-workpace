//! Ordre des pistes, historique, rebouclage.
//!
//! Volontairement dépourvu de volume, de pause et de normalisation : ces trois
//! réglages n'appartiennent pas au domaine, ils appartiennent au serveur. Chacun
//! les persiste sous une clé différente d'`env.json` (`VOLUME` pour la lecture
//! locale, `WEBRADIO_VOLUME` pour la radio) et les applique différemment — au
//! `Sink` d'un côté, échantillon par échantillon de l'autre. Les mutualiser
//! ferait que régler le volume de la radio changerait celui de la lecture locale.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use janus_library_nucleus::music::collect_tracks;
use janus_log_nucleus::log_info;

/// L'historique sert au diagnostic et à la navigation arrière, pas à l'archivage :
/// le borner évite qu'un serveur laissé allumé plusieurs jours n'accumule
/// indéfiniment des chemins.
pub const HISTORY_MAX: usize = 50;

#[derive(Default)]
pub struct Playlist {
    queue: VecDeque<PathBuf>,
    history: VecDeque<PathBuf>,
    current_file: Option<PathBuf>,
    /// Dossier de la playlist en cours, mémorisé pour le rebouclage.
    ///
    /// Le reconstruire depuis `current_file.parent()` — ce que faisait la version
    /// carte son — réduit la playlist au seul sous-dossier de la dernière piste
    /// jouée, puisque le parcours est récursif. Garder le dossier *demandé* évite
    /// ce rétrécissement.
    current_folder: Option<PathBuf>,
    /// Signal « la sortie doit rouvrir une piste ».
    ///
    /// Incrémenté aussi bien par un changement de playlist que par un saut avant ou
    /// arrière : dans les trois cas la sortie doit abandonner la piste en cours et
    /// dépiler la suivante, donc un seul compteur suffit. Un compteur plutôt qu'un
    /// booléen « il faut redémarrer », qu'il faudrait remettre à zéro — et dont la
    /// remise à zéro ouvrirait une course entre les deux fils.
    generation: u64,
}

impl Playlist {
    pub fn new() -> Self {
        Self::default()
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

    /// Demande à la sortie d'abandonner la piste en cours et de dépiler la suivante.
    ///
    /// L'historique est géré par [`Self::next_track`], que la sortie appellera : il
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

    /// Vide tout : file, piste courante et dossier mémorisé.
    ///
    /// Le dossier part avec le reste, sans quoi [`Self::has_next`] resterait vrai
    /// après un arrêt et la lecture repartirait toute seule au tour suivant.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.current_file = None;
        self.current_folder = None;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn has_previous(&self) -> bool {
        !self.history.is_empty()
    }

    /// Y a-t-il une suite ?
    ///
    /// Vrai dès qu'un dossier est chargé, même file vide : la lecture reboucle
    /// indéfiniment, donc il y a toujours une piste après.
    pub fn has_next(&self) -> bool {
        !self.queue.is_empty() || self.current_folder.is_some()
    }

    /// Chemin de la piste en cours, à sonder **après** avoir relâché le verrou.
    ///
    /// Lire les tags coûte plusieurs millisecondes ; le faire verrou en main
    /// bloquerait un producteur de flux, qui le reprend toutes les 192 ms, et se
    /// traduirait par un blanc pour tous les auditeurs.
    pub fn current_path(&self) -> Option<PathBuf> {
        self.current_file.clone()
    }

    pub fn current_music_name(&self) -> Option<String> {
        Self::file_name(self.current_file.as_ref())
    }

    pub fn previous_music_name(&self) -> Option<String> {
        Self::file_name(self.history.back())
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    fn file_name(path: Option<&PathBuf>) -> Option<String> {
        path?
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(nom: &str, pistes: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("janus_playlist_nucleus_test")
            .join(nom);
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
        let mut pl = Playlist::new();
        assert_eq!(pl.load_folder(&dir), 3);
    }

    #[test]
    fn un_dossier_vide_laisse_la_playlist_intacte() {
        let plein = fixture("plein", &["a.mp3"]);
        let vide = fixture("vide", &[]);

        let mut pl = Playlist::new();
        pl.load_folder(&plein);
        let generation = pl.generation();

        assert_eq!(pl.load_folder(&vide), 0);
        assert_eq!(pl.queue_len(), 1, "la playlist a été écrasée");
        assert_eq!(pl.generation(), generation, "génération incrémentée à tort");
    }

    #[test]
    fn changer_de_playlist_incremente_la_generation() {
        let dir = fixture("generation", &["a.mp3"]);
        let mut pl = Playlist::new();

        let avant = pl.generation();
        pl.load_folder(&dir);
        assert_ne!(pl.generation(), avant);
    }

    /// Le rebouclage doit repartir du dossier demandé, et non du dossier parent de
    /// la dernière piste jouée — sinon une piste rangée dans un sous-dossier y
    /// enfermerait la lecture.
    #[test]
    fn reboucle_sur_le_dossier_demande_pas_sur_le_sous_dossier() {
        let dir = fixture("reboucle", &["a.mp3"]);
        fs::write(dir.join("sous-dossier").join("b.mp3"), b"factice").unwrap();

        let mut pl = Playlist::new();
        assert_eq!(pl.load_folder(&dir), 2);

        pl.next_track().unwrap();
        pl.next_track().unwrap();
        assert_eq!(pl.queue_len(), 0);

        pl.next_track().unwrap();
        // Le rebouclage doit avoir retrouvé les deux pistes, pas seulement celles
        // du dossier de la dernière piste jouée.
        assert_eq!(pl.queue_len(), 1);
    }

    #[test]
    fn l_historique_reste_borne() {
        let dir = fixture("historique", &["a.mp3"]);
        let mut pl = Playlist::new();
        pl.load_folder(&dir);

        for _ in 0..(HISTORY_MAX * 2 + 10) {
            pl.next_track().unwrap();
        }
        assert!(pl.history_len() <= HISTORY_MAX);
    }

    /// Sans playlist chargée, rien à jouer : la sortie diffusera du silence plutôt
    /// que de se fermer.
    #[test]
    fn sans_playlist_il_n_y_a_pas_de_piste_suivante() {
        let mut pl = Playlist::new();
        assert!(pl.next_track().is_none());
        assert!(pl.current_music_name().is_none());
    }

    #[test]
    fn play_previous_sans_historique_ne_fait_rien() {
        let dir = fixture("prev_vide", &["a.mp3"]);
        let mut pl = Playlist::new();
        pl.load_folder(&dir);

        let generation = pl.generation();
        assert!(!pl.has_previous());
        assert!(!pl.play_previous());
        assert_eq!(
            pl.generation(),
            generation,
            "génération incrémentée alors que rien n'a bougé"
        );
    }

    /// Le comportement qui décide de la justesse de la navigation : après un
    /// aller-retour, on doit retomber sur la piste d'où l'on vient.
    #[test]
    fn next_puis_previous_revient_sur_la_meme_piste() {
        let dir = fixture("aller_retour", &["a.mp3", "b.mp3", "c.mp3"]);
        let mut pl = Playlist::new();
        pl.load_folder(&dir);

        let premiere = pl.next_track().unwrap();
        let deuxieme = pl.next_track().unwrap();
        assert_ne!(premiere, deuxieme);
        assert!(pl.has_previous());

        assert!(pl.play_previous());
        assert_eq!(pl.next_track().unwrap(), premiere);
    }

    /// Reculer deux fois doit reculer de deux pistes. Si `play_previous` clonait la
    /// piste courante au lieu de la retirer, `next_track` la réempilerait dans
    /// l'historique et l'on ferait du sur-place entre deux morceaux.
    #[test]
    fn deux_previous_consecutifs_reculent_bien_de_deux() {
        let dir = fixture("recul_double", &["a.mp3", "b.mp3", "c.mp3"]);
        let mut pl = Playlist::new();
        pl.load_folder(&dir);

        let p1 = pl.next_track().unwrap();
        let _p2 = pl.next_track().unwrap();
        let p3 = pl.next_track().unwrap();
        assert_eq!(pl.current_path().unwrap(), p3);

        assert!(pl.play_previous());
        let revenu2 = pl.next_track().unwrap();
        assert!(pl.play_previous());
        let revenu1 = pl.next_track().unwrap();

        assert_ne!(revenu2, revenu1, "sur-place entre deux pistes");
        assert_eq!(revenu1, p1);
    }

    #[test]
    fn has_next_est_vrai_des_qu_un_dossier_est_charge() {
        let dir = fixture("has_next", &["a.mp3"]);
        let mut pl = Playlist::new();
        assert!(!pl.has_next(), "aucun dossier chargé");

        pl.load_folder(&dir);
        assert!(pl.has_next());

        // File vidée, mais la lecture reboucle : il y a toujours une suite.
        pl.next_track().unwrap();
        assert_eq!(pl.queue_len(), 0);
        assert!(pl.has_next());
    }

    #[test]
    fn skip_next_signale_la_sortie_sans_toucher_a_la_file() {
        let dir = fixture("skip", &["a.mp3", "b.mp3"]);
        let mut pl = Playlist::new();
        pl.load_folder(&dir);

        let generation = pl.generation();
        let file_avant = pl.queue_len();
        pl.skip_next();

        assert_ne!(pl.generation(), generation);
        assert_eq!(pl.queue_len(), file_avant, "la file ne doit pas bouger");
    }

    /// Après un arrêt, plus rien ne doit repartir tout seul — d'où le dossier
    /// mémorisé qui part avec le reste.
    #[test]
    fn clear_coupe_aussi_le_rebouclage() {
        let dir = fixture("clear", &["a.mp3", "b.mp3"]);
        let mut pl = Playlist::new();
        pl.load_folder(&dir);
        pl.next_track().unwrap();

        pl.clear();
        assert!(!pl.has_next());
        assert!(pl.next_track().is_none());
        assert!(pl.current_music_name().is_none());
    }
}
