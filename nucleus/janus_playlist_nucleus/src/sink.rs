//! Le port de sortie, et l'enchaînement de piste qui va avec.

use std::path::{Path, PathBuf};

use janus_log_nucleus::log_error;

/// Pistes illisibles tolérées d'affilée avant de rendre la main.
///
/// Remplace la récursion de l'ancien `play_next`, qui se rappelait à chaque
/// fichier illisible et finissait par déborder la pile sur un dossier entièrement
/// corrompu.
pub const MAX_OPEN_ATTEMPTS: usize = 8;

/// La sortie audio d'un enfant, vue par le domaine.
///
/// Trois opérations suffisent parce que le domaine ne veut savoir qu'une chose :
/// « faut-il dépiler une piste de plus ? ». Comment les échantillons quittent le
/// processus — carte son, flux MP3, fichier — ne le regarde pas, et c'est ce qui
/// permet aux deux cadences existantes de coexister : un `rodio::Sink` consomme sa
/// source au rythme du périphérique, un producteur de flux est cadencé par bloc.
pub trait TrackSink {
    /// Ouvre `path` pour lecture, avec le gain de normalisation déjà résolu.
    ///
    /// Renvoie `false` si le fichier est illisible. C'est un booléen et non un
    /// `Result` parce que personne n'inspecte la cause : l'adaptateur la
    /// journalise avec le détail qu'il est seul à connaître (décodeur, format,
    /// périphérique), et l'appelant se contente de passer au fichier suivant.
    fn open(&mut self, path: &Path, gain: f32) -> bool;

    /// Plus rien à jouer : aucune piste ouverte, ou piste arrivée à son terme.
    fn is_exhausted(&self) -> bool;

    /// Abandonne la piste en cours.
    fn clear(&mut self);
}

/// Ouvre la prochaine piste jouable, en sautant celles qui échouent.
///
/// `next` fournit les chemins un par un, plutôt que de recevoir la
/// [`crate::Playlist`] directement : les deux serveurs ne prennent pas leur verrou
/// de la même façon — l'un tient l'état pendant tout l'enchaînement, l'autre le
/// relâche entre chaque tentative pour ne pas figer son producteur. Passer une
/// fermeture laisse ce choix à l'appelant.
///
/// Renvoie `false` quand il n'y a plus rien à jouer, ou après
/// [`MAX_OPEN_ATTEMPTS`] échecs consécutifs.
pub fn next_playable<S: TrackSink>(
    mut next: impl FnMut() -> Option<PathBuf>,
    sink: &mut S,
    gain: impl Fn(&Path) -> f32,
) -> bool {
    for _ in 0..MAX_OPEN_ATTEMPTS {
        let Some(path) = next() else {
            return false;
        };
        if sink.open(&path, gain(&path)) {
            return true;
        }
    }

    log_error(format!(
        "{MAX_OPEN_ATTEMPTS} pistes illisibles d'affilée — plus rien n'est joué"
    ));
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Sortie factice : refuse tout fichier dont le nom commence par `ko`.
    #[derive(Default)]
    struct SinkFactice {
        ouverte: Option<PathBuf>,
        gain: f32,
        tentatives: usize,
    }

    impl TrackSink for SinkFactice {
        fn open(&mut self, path: &Path, gain: f32) -> bool {
            self.tentatives += 1;
            if path.file_name().unwrap().to_string_lossy().starts_with("ko") {
                return false;
            }
            self.ouverte = Some(path.to_path_buf());
            self.gain = gain;
            true
        }
        fn is_exhausted(&self) -> bool {
            self.ouverte.is_none()
        }
        fn clear(&mut self) {
            self.ouverte = None;
        }
    }

    fn file(noms: &[&str]) -> impl FnMut() -> Option<PathBuf> {
        let mut restantes: Vec<PathBuf> = noms.iter().rev().map(PathBuf::from).collect();
        move || restantes.pop()
    }

    #[test]
    fn ouvre_la_premiere_piste_lisible() {
        let mut sink = SinkFactice::default();
        assert!(next_playable(file(&["ok1.mp3", "ok2.mp3"]), &mut sink, |_| 1.0));
        assert_eq!(sink.ouverte.unwrap(), PathBuf::from("ok1.mp3"));
    }

    #[test]
    fn saute_les_pistes_illisibles() {
        let mut sink = SinkFactice::default();
        assert!(next_playable(
            file(&["ko1.mp3", "ko2.mp3", "ok.mp3"]),
            &mut sink,
            |_| 1.0
        ));
        assert_eq!(sink.ouverte.unwrap(), PathBuf::from("ok.mp3"));
        assert_eq!(sink.tentatives, 3);
    }

    /// Le garde-fou qui remplace la récursion : un dossier entièrement corrompu
    /// doit rendre la main, pas dérouler la pile.
    #[test]
    fn abandonne_apres_le_plafond_de_tentatives() {
        let mut sink = SinkFactice::default();
        let noms: Vec<String> = (0..100).map(|i| format!("ko{i}.mp3")).collect();
        let refs: Vec<&str> = noms.iter().map(String::as_str).collect();

        assert!(!next_playable(file(&refs), &mut sink, |_| 1.0));
        assert_eq!(sink.tentatives, MAX_OPEN_ATTEMPTS);
        assert!(sink.is_exhausted());
    }

    #[test]
    fn une_file_vide_ne_tente_rien() {
        let mut sink = SinkFactice::default();
        assert!(!next_playable(file(&[]), &mut sink, |_| 1.0));
        assert_eq!(sink.tentatives, 0);
    }

    #[test]
    fn le_gain_est_transmis_a_la_sortie() {
        let mut sink = SinkFactice::default();
        assert!(next_playable(file(&["ok.mp3"]), &mut sink, |_| 2.5));
        assert_eq!(sink.gain, 2.5);
    }
}
