use crate::logger::{log_error, log_info};
use ebur128::{EbuR128, Mode};
use rodio::{Decoder, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Manages audio normalization cache and calculations.
pub struct NormalizationManager {
    cache: Arc<Mutex<HashMap<String, f32>>>,
    target_lufs: f64,
}

impl NormalizationManager {
    pub fn new(target_lufs: f64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            target_lufs,
        }
    }

    /// Default configuration (-14.0 LUFS)
    pub fn default() -> Self {
        Self::new(-14.0)
    }

    /// Retrieve or compute normalization gain for a file.
    pub fn get_or_compute_gain(&self, path: &Path) -> f32 {
        let path_str = path.to_string_lossy().to_string();

        // 1. Check cache
        if let Ok(cache) = self.cache.lock() {
            if let Some(&gain) = cache.get(&path_str) {
                return gain;
            }
        }

        // 2. Compute
        log_info(format!("Analyse EBU R128 en cours pour : {}", path_str));
        let gain = match self.scan_file_loudness(path) {
            Ok(measured_lufs) => {
                if measured_lufs == f64::NEG_INFINITY {
                    1.0 // Silence
                } else {
                    let diff = self.target_lufs - measured_lufs;
                    let linear_gain = 10.0f64.powf(diff / 20.0);
                    // Clamp max gain (+15dB ~5.6x)
                    let final_gain = linear_gain.min(5.6) as f32;
                    log_info(format!(
                        "Résultat : {:.1} LUFS. Gain appliqué : {:.2} dB (x{:.2})",
                        measured_lufs, diff, final_gain
                    ));
                    final_gain
                }
            }
            Err(e) => {
                log_error(format!("Erreur analyse loudness pour {} : {}", path_str, e));
                1.0
            }
        };

        // 3. Update cache
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path_str, gain);
        }

        gain
    }

    fn scan_file_loudness(&self, path: &Path) -> Result<f64, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let source = Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?;

        let channels = source.channels() as u32;
        let sample_rate = source.sample_rate();

        let mut scanner = EbuR128::new(channels, sample_rate, Mode::I)
            .map_err(|_| "Impossible d'initialiser EbuR128".to_string())?;

        // Rodio samples are already normalized f32 if using convert_samples() but Decoder implements Iterator<Item=i16> by default usually?
        // Actually Decoder implements Source without strict type until you iterate.
        // We use convert_samples() to get f32 iterator.
        let samples: Vec<f32> = source.convert_samples().collect();

        scanner
            .add_frames_f32(&samples)
            .map_err(|_| "Erreur lors de l'ajout des frames".to_string())?;

        scanner
            .loudness_global()
            .map_err(|_| "Erreur calcul loudness global".to_string())
    }
}
