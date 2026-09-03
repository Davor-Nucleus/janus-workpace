//! Ce qu'il faut pour exploiter une bibliotheque musicale sur disque :
//! decouvrir les playlists, lire les tags, mesurer le niveau.
//!
//! Separe de `janus_playlist_nucleus`, qui decide *quoi* jouer : ici on ne fait
//! que lire des fichiers. Un enfant qui veut seulement normaliser un son
//! (PhonosCore) prend cette brique sans prendre la playlist.

pub mod audio;
pub mod metadata;
pub mod music;
