//! Lecture des tags ID3/Vorbis et de la pochette embarquée.
//!
//! Exposé sous forme de **fonction libre prenant un chemin**, et non de méthode
//! sur un état de lecteur : sonder un fichier coûte plusieurs millisecondes, et
//! l'appelant doit pouvoir le faire après avoir relâché son verrou. WebRadioCore
//! en dépend — son moteur reprend le verrou d'état toutes les 192 ms, et le
//! bloquer se traduirait par un blanc pour tous les auditeurs.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use std::fs::File;
use std::path::Path;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;

#[derive(Serialize)]
pub struct MusicMetadata {
    pub filename: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<String>,
    pub cover_art: Option<String>,
}

/// Lit les métadonnées de `path`.
///
/// Ne renvoie `None` que si le chemin n'a pas de nom de fichier exploitable : un
/// fichier illisible ou dépourvu de tags rend une structure ne portant que le nom,
/// pour qu'un affichage « en cours de lecture » reste utilisable.
pub fn read_metadata(path: &Path) -> Option<MusicMetadata> {
    let filename = path.file_name().and_then(|n| n.to_str())?.to_string();

    let mut meta = MusicMetadata {
        filename,
        title: None,
        artist: None,
        album: None,
        date: None,
        cover_art: None,
    };

    let Ok(file) = File::open(path) else {
        return Some(meta);
    };

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension() {
        hint.with_extension(&ext.to_string_lossy());
    }

    let mut probed = match symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(_) => return Some(meta),
    };

    // probed.metadata : Option<Metadata<'_>> → .current() → Option<&MetadataRevision>
    // probed.format   : Metadata<'_>         → .current() → Option<&MetadataRevision>
    // Les deux doivent être stockés en bindings pour que les lifetimes tiennent.
    let probe_meta = probed.metadata.get();
    let probe_rev = probe_meta.as_ref().and_then(|m| m.current());

    let fmt_meta = probed.format.metadata();
    let format_rev = fmt_meta.current();

    for rev in [probe_rev, format_rev].into_iter().flatten() {
        if meta.title.is_some() && meta.artist.is_some() && meta.cover_art.is_some() {
            break;
        }
        fill_from_revision(rev, &mut meta);
    }

    Some(meta)
}

fn fill_from_revision(rev: &MetadataRevision, meta: &mut MusicMetadata) {
    for tag in rev.tags() {
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) => {
                meta.title.get_or_insert_with(|| tag.value.to_string());
            }
            Some(StandardTagKey::Artist) => {
                meta.artist.get_or_insert_with(|| tag.value.to_string());
            }
            Some(StandardTagKey::Album) => {
                meta.album.get_or_insert_with(|| tag.value.to_string());
            }
            Some(StandardTagKey::Date) => {
                meta.date.get_or_insert_with(|| tag.value.to_string());
            }
            _ => {}
        }
    }
    if meta.cover_art.is_none() {
        if let Some(visual) = rev.visuals().first() {
            if let Some(mime) = sanitize_cover_mime(&visual.media_type) {
                let encoded = STANDARD.encode(&*visual.data);
                meta.cover_art = Some(format!("data:{};base64,{}", mime, encoded));
            }
        }
    }
}

/// Types d'images acceptés pour une pochette embarquée.
const COVER_MIME_ALLOWLIST: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Valide le `media_type` d'une pochette embarquée avant de le coller dans une URL `data:`.
///
/// Ce champ vient du tag du fichier audio, donc d'une source non maîtrisée : un mp3
/// récupéré ailleurs peut y placer n'importe quoi. Sans filtre, la chaîne ressortait
/// telle quelle dans `data:{media_type};base64,...`, et un guillemet suffisait à sortir
/// de l'attribut `src` côté overlay. La liste blanche est la première barrière ; la
/// construction par API DOM dans `music_current.html` est la seconde.
///
/// Tolère les variantes de casse et les paramètres (`image/png; charset=binary`), et
/// accepte les alias courants que produisent certains encodeurs.
fn sanitize_cover_mime(media_type: &str) -> Option<&'static str> {
    let base = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let normalized = match base.as_str() {
        "image/jpg" | "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        other => other,
    };

    COVER_MIME_ALLOWLIST
        .iter()
        .find(|allowed| **allowed == normalized)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepte_les_types_usuels() {
        assert_eq!(sanitize_cover_mime("image/png"), Some("image/png"));
        assert_eq!(sanitize_cover_mime("image/jpeg"), Some("image/jpeg"));
        assert_eq!(sanitize_cover_mime("image/gif"), Some("image/gif"));
        assert_eq!(sanitize_cover_mime("image/webp"), Some("image/webp"));
    }

    #[test]
    fn normalise_casse_alias_et_parametres() {
        assert_eq!(sanitize_cover_mime("IMAGE/PNG"), Some("image/png"));
        assert_eq!(sanitize_cover_mime("image/jpg"), Some("image/jpeg"));
        assert_eq!(sanitize_cover_mime("  image/png  "), Some("image/png"));
        assert_eq!(
            sanitize_cover_mime("image/png; charset=binary"),
            Some("image/png")
        );
    }

    #[test]
    fn refuse_une_sortie_d_attribut() {
        // Le motif exact qui permettait d'echapper au src="" de l'overlay.
        assert_eq!(sanitize_cover_mime("image/png\" onerror=\"alert(1)"), None);
        assert_eq!(sanitize_cover_mime("image/png'><script>"), None);
    }

    #[test]
    fn refuse_les_types_hors_liste() {
        assert_eq!(sanitize_cover_mime("text/html"), None);
        assert_eq!(sanitize_cover_mime("image/svg+xml"), None); // SVG = script
        assert_eq!(sanitize_cover_mime(""), None);
    }

    #[test]
    fn la_valeur_retournee_ne_vient_jamais_de_l_entree() {
        // La sortie est toujours un &'static str de la liste blanche : meme sur une
        // entree valide, aucun octet de l'entree ne transite vers l'URL data:.
        let sortie = sanitize_cover_mime("image/PNG; x=1").unwrap();
        assert!(COVER_MIME_ALLOWLIST.contains(&sortie));
    }

    /// Un fichier absent ne doit pas faire disparaître l'entrée « en cours de
    /// lecture » : on rend le nom seul.
    #[test]
    fn un_fichier_absent_rend_le_nom_seul() {
        let meta = read_metadata(Path::new("n-existe-pas.mp3")).unwrap();
        assert_eq!(meta.filename, "n-existe-pas.mp3");
        assert!(meta.title.is_none());
        assert!(meta.cover_art.is_none());
    }
}
