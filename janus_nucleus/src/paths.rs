//! Confinement des chemins fournis par une requête HTTP.

use std::path::{Path, PathBuf};

/// Résout `candidate` **sous** `base`, ou renvoie `None`.
///
/// `Path::join` remplace intégralement la base quand l'argument est absolu
/// (`base.join("C:/Windows")` == `C:/Windows`), et `..` permet de remonter :
/// sans ce garde-fou, un paramètre de requête donnait accès à tout le disque.
/// On canonicalise les deux côtés avant de comparer, sinon `starts_with`
/// travaillerait sur des chemins non résolus et laisserait passer les `..`.
///
/// Renvoie `None` aussi quand la cible n'existe pas : l'appelant doit répondre la
/// même chose dans les deux cas, faute de quoi la distinction rend le service
/// bavard sur l'existence de chemins hors de `base`.
pub fn resolve_within(base: &Path, candidate: &str) -> Option<PathBuf> {
    let base = base.canonicalize().ok()?;
    let resolved = base.join(candidate).canonicalize().ok()?;
    resolved.starts_with(&base).then_some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join("janus_nucleus_paths_test");
        fs::create_dir_all(dir.join("musique")).unwrap();
        fs::write(dir.join("musique").join("a.mp3"), b"x").unwrap();
        fs::write(dir.join("secret.txt"), b"x").unwrap();
        dir
    }

    #[test]
    fn accepte_un_sous_dossier() {
        let base = fixture();
        assert!(resolve_within(&base, "musique").is_some());
    }

    #[test]
    fn accepte_un_fichier_imbrique() {
        let base = fixture();
        assert!(resolve_within(&base, "musique/a.mp3").is_some());
    }

    #[test]
    fn refuse_la_remontee_par_points() {
        let base = fixture();
        // `secret.txt` existe, mais il est au-dessus de la base visée.
        assert!(resolve_within(&base.join("musique"), "../secret.txt").is_none());
    }

    #[test]
    fn refuse_un_chemin_absolu() {
        let base = fixture();
        let absolu = base.join("secret.txt").to_string_lossy().to_string();
        assert!(resolve_within(&base.join("musique"), &absolu).is_none());
    }

    #[test]
    fn refuse_une_cible_inexistante() {
        let base = fixture();
        assert!(resolve_within(&base, "n-existe-pas").is_none());
    }
}
