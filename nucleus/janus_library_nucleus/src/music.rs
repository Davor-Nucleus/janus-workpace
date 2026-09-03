//! Découverte des playlists sur disque.
//!
//! JanusCore et WebRadioCore lisent le même arbre `public/music/` avec les mêmes
//! règles — un sous-dossier par playlist, parcours récursif, lecture aléatoire.
//! Le filtre d'extensions décide de ce qui est jouable : quand les deux copies
//! divergent, un fichier apparaît dans une playlist et pas dans l'autre sans que
//! rien ne le signale.

use rand::seq::SliceRandom;
use std::path::{Path, PathBuf};

/// Racine des playlists, relative au répertoire de travail du processus.
pub const MUSIC_ROOT: &str = "./public/music";

/// Extensions retenues pour une playlist.
///
/// Volontairement plus étroit que ce que sait décoder Symphonia : les formats
/// déclarés ailleurs (AAC, MP4) n'ont jamais été acceptés ici, et les ajouter
/// serait un changement de comportement, pas un nettoyage.
pub const PLAYLIST_EXTENSIONS: [&str; 3] = ["mp3", "wav", "flac"];

/// Noms des sous-dossiers directs de `root`, triés.
///
/// Le tri rend l'ordre stable d'un appel à l'autre : `read_dir` ne le garantit
/// pas, et une liste qui change de place entre deux rafraîchissements est
/// désagréable dans une interface.
pub fn list_folders(root: impl AsRef<Path>) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut folders: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    folders.sort();
    folders
}

/// Pistes jouables de `folder`, parcours récursif, mélangées.
///
/// Renvoie un vecteur vide si le dossier est absent ou ne contient rien de
/// jouable ; c'est à l'appelant de décider quoi en faire — les deux serveurs ont
/// des politiques différentes sur ce cas.
pub fn collect_tracks(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_playlist_track(e.path()))
        .map(|e| e.into_path())
        .collect();

    files.shuffle(&mut rand::thread_rng());
    files
}

/// Le fichier porte-t-il une extension jouable ? Insensible à la casse.
pub fn is_playlist_track(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PLAYLIST_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("janus_library_nucleus_music_test").join(nom);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("rock").join("live")).unwrap();
        fs::create_dir_all(dir.join("jazz")).unwrap();
        fs::write(dir.join("rock").join("a.mp3"), b"x").unwrap();
        fs::write(dir.join("rock").join("b.FLAC"), b"x").unwrap();
        fs::write(dir.join("rock").join("live").join("c.wav"), b"x").unwrap();
        fs::write(dir.join("rock").join("notes.txt"), b"x").unwrap();
        fs::write(dir.join("rock").join("d.m4a"), b"x").unwrap();
        fs::write(dir.join("racine.mp3"), b"x").unwrap();
        dir
    }

    #[test]
    fn liste_les_sous_dossiers_tries_sans_les_fichiers() {
        let dir = fixture("dossiers");
        assert_eq!(list_folders(&dir), vec!["jazz".to_string(), "rock".to_string()]);
    }

    #[test]
    fn un_dossier_absent_rend_une_liste_vide() {
        assert!(list_folders("n-existe-vraiment-pas").is_empty());
    }

    #[test]
    fn collecte_recursivement_et_filtre_les_extensions() {
        let dir = fixture("pistes");
        let pistes = collect_tracks(&dir.join("rock"));
        let mut noms: Vec<String> = pistes
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        noms.sort();

        // `live/c.wav` prouve la récursion ; `.txt` et `.m4a` sont écartés.
        assert_eq!(noms, vec!["a.mp3", "b.FLAC", "c.wav"]);
    }

    #[test]
    fn l_extension_est_insensible_a_la_casse() {
        assert!(is_playlist_track(Path::new("x.MP3")));
        assert!(is_playlist_track(Path::new("x.FlAc")));
        assert!(!is_playlist_track(Path::new("x.ogg")));
        assert!(!is_playlist_track(Path::new("sans_extension")));
    }

    #[test]
    fn un_dossier_sans_audio_rend_un_vecteur_vide() {
        let dir = fixture("vide");
        assert!(collect_tracks(&dir.join("jazz")).is_empty());
    }
}
