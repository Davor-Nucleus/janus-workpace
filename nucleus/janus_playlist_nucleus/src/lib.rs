//! Capacite « lecture de dossiers de musique en aleatoire ».
//!
//! Le domaine decide **quoi** jouer ensuite ; il ne sait pas produire de son. La
//! sortie est fournie par l'enfant sous forme d'un [`TrackSink`], parce que les
//! deux sorties existantes sont irreconciliables : une carte son tire les
//! echantillons a son propre rythme, un flux MP3 est pousse par blocs cadences.
//! Ce qu'elles partagent est exactement ce qui vit ici -- l'ordre des pistes,
//! l'historique, le rebouclage et la tolerance aux fichiers illisibles.

mod gain;
mod playlist;
mod sink;

pub use gain::{resolve_gain, warm_gain, warm_next};
pub use playlist::{Playlist, HISTORY_MAX};
pub use sink::{next_playable, TrackSink, MAX_OPEN_ATTEMPTS};

pub use janus_library_nucleus::music::MUSIC_ROOT;

/// Playlists disponibles sous [`MUSIC_ROOT`].
///
/// Mutualise parce que les deux serveurs l'exposaient a l'identique : sans elle,
/// rien ne permet de decouvrir une valeur valide pour le parametre `folder`.
pub fn folders() -> Vec<String> {
    janus_library_nucleus::music::list_folders(MUSIC_ROOT)
}
