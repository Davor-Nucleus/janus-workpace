//! Encodage MP3 via LAME.

use mp3lame_encoder::{
    max_required_buffer_size, Bitrate, Builder, Encoder, FlushNoGap, InterleavedPcm, Quality,
};

/// Marge de sortie pour le `flush` final.
///
/// `max_required_buffer_size(0)` vaut 7200, la réserve fixe que LAME exige pour
/// écouler ses trames en attente.
const FLUSH_BUFFER_SIZE: usize = max_required_buffer_size(0);

// Attention en modifiant `encode_interleaved` / `flush` : les variantes
// `encode_to_vec` et `flush_to_vec` du crate sont un piège. Elles appellent
// `spare_capacity_mut()` **sans réserver au préalable**, donc sur un `Vec` neuf
// elles passent un buffer de taille zéro à LAME — qui traite une taille nulle
// comme « le buffer est assez grand, ne pas vérifier » et écrit hors limites.
// Le résultat est une corruption de tas silencieuse, observée ici en
// STATUS_ACCESS_VIOLATION. On réserve donc explicitement avant chaque appel.

/// Débits acceptés par LAME, du plus faible au plus élevé.
const BITRATES: [(u16, Bitrate); 16] = [
    (8, Bitrate::Kbps8),
    (16, Bitrate::Kbps16),
    (24, Bitrate::Kbps24),
    (32, Bitrate::Kbps32),
    (40, Bitrate::Kbps40),
    (48, Bitrate::Kbps48),
    (64, Bitrate::Kbps64),
    (80, Bitrate::Kbps80),
    (96, Bitrate::Kbps96),
    (112, Bitrate::Kbps112),
    (128, Bitrate::Kbps128),
    (160, Bitrate::Kbps160),
    (192, Bitrate::Kbps192),
    (224, Bitrate::Kbps224),
    (256, Bitrate::Kbps256),
    (320, Bitrate::Kbps320),
];

/// Débit LAME le plus proche de `kbps`.
///
/// La configuration est un entier libre alors que LAME n'accepte que seize
/// valeurs ; refuser la configuration pour un « 200 » serait pénible pour rien.
pub fn nearest_bitrate(kbps: u16) -> Bitrate {
    BITRATES
        .iter()
        .min_by_key(|(v, _)| v.abs_diff(kbps))
        .map(|(_, b)| *b)
        .unwrap_or(Bitrate::Kbps192)
}

/// Encodeur MP3 à format fixe.
///
/// LAME fige la fréquence et le nombre de canaux au `build()` : c'est pour cette
/// raison que l'appelant doit rééchantillonner vers un format unique en amont
/// (voir `rodio::source::UniformSourceIterator`).
pub struct Mp3Encoder {
    inner: Encoder,
}

impl Mp3Encoder {
    pub fn new(sample_rate: u32, channels: u16, bitrate_kbps: u16) -> Result<Self, String> {
        let builder = Builder::new().ok_or_else(|| "LAME : allocation impossible".to_string())?;

        let inner = builder
            .with_num_channels(channels as u8)
            .map_err(|e| format!("LAME : nombre de canaux {channels} refusé ({e:?})"))?
            .with_sample_rate(sample_rate)
            .map_err(|e| format!("LAME : fréquence {sample_rate} Hz refusée ({e:?})"))?
            .with_brate(nearest_bitrate(bitrate_kbps))
            .map_err(|e| format!("LAME : débit {bitrate_kbps} kbps refusé ({e:?})"))?
            // `Good` est le compromis qui tient largement le temps réel sur un CPU
            // de bureau. Descendre vers `Nice`/`Decent` si la machine peine.
            .with_quality(Quality::Good)
            .map_err(|e| format!("LAME : qualité refusée ({e:?})"))?
            .build()
            .map_err(|e| format!("LAME : construction de l'encodeur échouée ({e:?})"))?;

        Ok(Self { inner })
    }

    /// Encode un bloc de PCM `i16` entrelacé.
    ///
    /// Le retour peut être vide : LAME accumule les échantillons jusqu'à pouvoir
    /// sortir une trame complète. L'appelant publie simplement ce qu'il reçoit.
    pub fn encode_interleaved(&mut self, pcm: &[i16]) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(max_required_buffer_size(pcm.len()));
        let written = self
            .inner
            .encode(InterleavedPcm(pcm), out.spare_capacity_mut())
            .map_err(|e| format!("LAME : encodage échoué ({e:?})"))?;
        // SAFETY : `encode` renvoie le nombre d'octets qu'il vient d'initialiser,
        // dans la limite de la capacité réservée juste au-dessus.
        unsafe { out.set_len(written) };
        Ok(out)
    }

    /// Vide les échantillons encore retenus par LAME.
    ///
    /// `FlushNoGap` complète la dernière trame sans insérer le silence de fin
    /// habituel — approprié pour un flux continu, où rien ne « finit » vraiment.
    pub fn flush(&mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(FLUSH_BUFFER_SIZE);
        let written = self
            .inner
            .flush::<FlushNoGap>(out.spare_capacity_mut())
            .map_err(|e| format!("LAME : flush échoué ({e:?})"))?;
        // SAFETY : idem, borné par la capacité réservée.
        unsafe { out.set_len(written) };
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CHUNK_FRAMES, CHUNK_SAMPLES, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE};

    #[test]
    fn choisit_le_debit_le_plus_proche() {
        assert!(matches!(nearest_bitrate(192), Bitrate::Kbps192));
        assert!(matches!(nearest_bitrate(200), Bitrate::Kbps192));
        assert!(matches!(nearest_bitrate(0), Bitrate::Kbps8));
        assert!(matches!(nearest_bitrate(9_999), Bitrate::Kbps320));
    }

    /// Encoder du silence doit produire de vraies trames MP3 : c'est exactement ce
    /// que le moteur diffuse quand aucune playlist n'est chargée.
    #[test]
    fn encode_du_silence_en_trames_valides() {
        let mut enc = Mp3Encoder::new(OUTPUT_SAMPLE_RATE, OUTPUT_CHANNELS, 192).unwrap();

        let silence = vec![0i16; CHUNK_SAMPLES];
        let mut octets = Vec::new();
        // Une seconde d'audio, pour dépasser largement le tampon interne de LAME.
        for _ in 0..6 {
            octets.extend(enc.encode_interleaved(&silence).unwrap());
        }
        octets.extend(enc.flush().unwrap());

        assert!(!octets.is_empty(), "aucune trame produite");
        // Synchro MPEG : onze bits à 1, soit 0xFF puis les trois bits de poids fort
        // de l'octet suivant.
        assert_eq!(octets[0], 0xFF, "pas de synchro en tête de flux");
        assert_eq!(octets[1] & 0xE0, 0xE0, "synchro MPEG incomplète");
    }

    /// Le vrai critère : ce que l'on publie doit être décodable par un client, au
    /// format annoncé. Un flux qui « ressemble » à du MP3 mais sort en mono ou en
    /// 44,1 kHz jouerait à la mauvaise vitesse chez l'auditeur.
    #[test]
    fn le_flux_produit_est_decodable_au_format_annonce() {
        use rodio::Source;

        let mut enc = Mp3Encoder::new(OUTPUT_SAMPLE_RATE, OUTPUT_CHANNELS, 192).unwrap();

        // Une seconde d'une onde carrée douce : du silence pur se compresse en
        // trames minuscules et testerait moins bien le chemin d'encodage.
        let mut octets = Vec::new();
        for bloc in 0..(OUTPUT_SAMPLE_RATE as usize / CHUNK_FRAMES + 1) {
            let pcm: Vec<i16> = (0..CHUNK_SAMPLES)
                .map(|i| {
                    let n = bloc * CHUNK_SAMPLES + i;
                    if (n / 128) % 2 == 0 { 8_000 } else { -8_000 }
                })
                .collect();
            octets.extend(enc.encode_interleaved(&pcm).unwrap());
        }
        octets.extend(enc.flush().unwrap());

        let decodeur = rodio::Decoder::new(std::io::Cursor::new(octets))
            .expect("le flux produit n'est pas un MP3 décodable");

        assert_eq!(decodeur.sample_rate(), OUTPUT_SAMPLE_RATE);
        assert_eq!(decodeur.channels(), OUTPUT_CHANNELS);

        // Le décodage doit rendre à peu près la seconde encodée. Les bornes sont
        // larges : encodeur et décodeur ajoutent chacun leur délai d'amorçage.
        let frames = decodeur.count() / OUTPUT_CHANNELS as usize;
        assert!(
            frames > OUTPUT_SAMPLE_RATE as usize / 2,
            "seulement {frames} frames décodées"
        );
    }
}
