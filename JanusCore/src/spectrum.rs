//! Spectre de la musique en cours, pour l'overlay `/music-visualizer`.
//!
//! [`Tap`] s'intercale entre le décodeur et le `Sink` : il laisse passer chaque
//! échantillon tel quel et en garde une copie pour l'analyse. Le spectre est donc
//! celui de ce qui part vers la carte son, **avant** le volume : baisser la musique
//! ne vide pas les barres, et un volume à zéro les laisse bouger.
//!
//! L'analyse tourne sur le fil audio de rodio, une fenêtre toutes les 1024 trames
//! (~43 par seconde à 44,1 kHz). Une FFT de 2048 points y coûte quelques dizaines de
//! microsecondes : rien qui puisse faire craquer le son.

use std::f32::consts::PI;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::Source;

/// Nombre de bandes publiées. L'overlay les regroupe ou les interpole selon le
/// nombre de barres réglé dans `/effects-config`.
pub const BANDS: usize = 64;

/// Taille de la fenêtre d'analyse, en trames. 2048 à 44,1 kHz : 46 ms, assez pour
/// séparer les graves (21,5 Hz par case).
const FFT_SIZE: usize = 2048;

/// Pas entre deux analyses : la moitié de la fenêtre, qui se chevauchent donc.
const HOP: usize = FFT_SIZE / 2;

/// Bornes de l'échelle logarithmique des bandes.
const MIN_HZ: f32 = 40.0;
const MAX_HZ: f32 = 16_000.0;

/// Plage affichée : une sinusoïde pleine échelle vaut 0 dB, le silence visuel
/// commence à -60 dB.
const FLOOR_DB: f32 = -60.0;

/// Au-delà, un spectre est périmé (musique en pause ou arrêtée) et vaut du silence.
pub const MAX_AGE: Duration = Duration::from_millis(250);

/// Transformée de Fourier en place, radix 2. `re.len()` doit être une puissance de 2.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

/// Niveaux par bande (0 à 1) d'une fenêtre de `FFT_SIZE` échantillons mono.
pub fn bands(samples: &[f32], sample_rate: u32) -> [f32; BANDS] {
    let n = samples.len();
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        // Fenêtre de Hann : sans elle, les bords de la fenêtre étalent chaque note
        // sur tout le spectre.
        .map(|(i, s)| s * (0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);

    // Une sinusoïde d'amplitude 1, fenêtrée par Hann, culmine à n/4.
    let scale = 4.0 / n as f32;
    let bin_hz = sample_rate as f32 / n as f32;
    let top = MAX_HZ.min(sample_rate as f32 / 2.0);
    let ratio = top / MIN_HZ;

    let mut out = [0.0; BANDS];
    for (band, level) in out.iter_mut().enumerate() {
        let lo = MIN_HZ * ratio.powf(band as f32 / BANDS as f32);
        let hi = MIN_HZ * ratio.powf((band + 1) as f32 / BANDS as f32);
        // Une bande plus étroite qu'une case (les graves) prend au moins une case.
        let first = (lo / bin_hz).floor() as usize;
        let last = ((hi / bin_hz).ceil() as usize).max(first + 1).min(n / 2);

        let peak = (first..last)
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * scale)
            .fold(0.0f32, f32::max);
        let db = 20.0 * peak.max(1e-9).log10();
        *level = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
    }
    out
}

/// Mélange les canaux en mono et produit un spectre toutes les [`HOP`] trames.
pub struct Analyzer {
    window: Vec<f32>,
    frame_sum: f32,
    frame_channels: u16,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self {
            window: Vec::with_capacity(FFT_SIZE),
            frame_sum: 0.0,
            frame_channels: 0,
        }
    }
}

impl Analyzer {
    /// Ajoute un échantillon entrelacé. Rend un spectre quand une fenêtre est pleine.
    pub fn push(&mut self, sample: f32, channels: u16, sample_rate: u32) -> Option<[f32; BANDS]> {
        self.frame_sum += sample;
        self.frame_channels += 1;
        if self.frame_channels < channels.max(1) {
            return None;
        }

        self.window.push(self.frame_sum / f32::from(self.frame_channels));
        self.frame_sum = 0.0;
        self.frame_channels = 0;

        if self.window.len() < FFT_SIZE {
            return None;
        }
        let levels = bands(&self.window, sample_rate);
        self.window.drain(..HOP);
        Some(levels)
    }
}

/// Dernier spectre calculé, partagé entre le fil audio et les WebSockets.
#[derive(Default)]
pub struct Spectrum {
    latest: Mutex<Option<([f32; BANDS], Instant)>>,
}

impl Spectrum {
    pub fn publish(&self, levels: [f32; BANDS], at: Instant) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some((levels, at));
    }

    /// Niveaux en octets (0 à 255) pour le WebSocket ; que des zéros si rien n'a
    /// été analysé depuis [`MAX_AGE`] — pause, arrêt, fin de playlist.
    pub fn levels(&self, now: Instant) -> Vec<u8> {
        let latest = *self.latest.lock().unwrap_or_else(|e| e.into_inner());
        match latest {
            Some((levels, at)) if now.saturating_duration_since(at) <= MAX_AGE => {
                levels.iter().map(|l| (l * 255.0).round() as u8).collect()
            }
            _ => vec![0; BANDS],
        }
    }
}

/// Source transparente qui alimente [`Spectrum`] au passage.
pub struct Tap<S> {
    inner: S,
    analyzer: Analyzer,
    spectrum: Arc<Spectrum>,
}

impl<S> Tap<S> {
    pub fn new(inner: S, spectrum: Arc<Spectrum>) -> Self {
        Self {
            inner,
            analyzer: Analyzer::default(),
            spectrum,
        }
    }
}

impl<S: Source<Item = i16>> Iterator for Tap<S> {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        let sample = self.inner.next()?;
        let channels = self.inner.channels();
        let rate = self.inner.sample_rate();
        if let Some(levels) = self.analyzer.push(f32::from(sample) / 32768.0, channels, rate) {
            self.spectrum.publish(levels, Instant::now());
        }
        Some(sample)
    }
}

impl<S: Source<Item = i16>> Source for Tap<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn sine(hz: f32, amplitude: f32) -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|i| amplitude * (2.0 * PI * hz * i as f32 / RATE as f32).sin())
            .collect()
    }

    /// Bande dont la plage couvre `hz`, calculée comme dans `bands`.
    fn band_of(hz: f32) -> usize {
        let ratio = MAX_HZ / MIN_HZ;
        ((hz / MIN_HZ).ln() / ratio.ln() * BANDS as f32).floor() as usize
    }

    #[test]
    fn une_sinusoide_pleine_echelle_remplit_sa_bande_et_elle_seule() {
        let levels = bands(&sine(1000.0, 1.0), RATE);
        let loudest = (0..BANDS).max_by(|&a, &b| levels[a].total_cmp(&levels[b])).unwrap();

        assert_eq!(loudest, band_of(1000.0));
        assert!(levels[loudest] > 0.95, "pic à {}", levels[loudest]);
        // Loin de la note, le spectre retombe : la fenêtre de Hann limite les fuites.
        assert!(levels[band_of(100.0)] < 0.2, "graves à {}", levels[band_of(100.0)]);
        assert!(levels[band_of(8000.0)] < 0.2, "aigus à {}", levels[band_of(8000.0)]);
    }

    #[test]
    fn un_son_plus_faible_donne_des_barres_plus_basses() {
        let loud = bands(&sine(440.0, 1.0), RATE)[band_of(440.0)];
        let quiet = bands(&sine(440.0, 0.01), RATE)[band_of(440.0)]; // -40 dB
        assert!((loud - quiet - 40.0 / 60.0).abs() < 0.05, "{loud} contre {quiet}");
    }

    #[test]
    fn le_silence_ne_leve_aucune_barre() {
        assert!(bands(&vec![0.0; FFT_SIZE], RATE).iter().all(|&l| l == 0.0));
    }

    #[test]
    fn l_analyseur_melange_les_canaux_et_publie_a_chaque_demi_fenetre() {
        let mut analyzer = Analyzer::default();
        let mut produced = 0;
        // Stéréo : deux échantillons par trame, 3 × HOP trames.
        for _ in 0..3 * HOP * 2 {
            if analyzer.push(0.0, 2, RATE).is_some() {
                produced += 1;
            }
        }
        // Première fenêtre pleine à 2 × HOP trames, puis une tous les HOP.
        assert_eq!(produced, 2);
    }

    #[test]
    fn la_prise_laisse_passer_le_son_intact() {
        let samples: Vec<i16> = (0..5000).map(|i| (i % 300) as i16 - 150).collect();
        let spectrum = Arc::new(Spectrum::default());
        let tap = Tap::new(rodio::buffer::SamplesBuffer::new(2, RATE, samples.clone()), spectrum.clone());

        assert_eq!(tap.channels(), 2);
        assert_eq!(tap.collect::<Vec<i16>>(), samples);
        // 2500 trames stéréo : au moins une fenêtre analysée.
        assert!(spectrum.latest.lock().unwrap().is_some());
    }

    #[test]
    fn un_spectre_perime_vaut_du_silence() {
        let spectrum = Spectrum::default();
        let t0 = Instant::now();
        spectrum.publish([1.0; BANDS], t0);

        assert_eq!(spectrum.levels(t0 + Duration::from_millis(100)), vec![255; BANDS]);
        assert_eq!(spectrum.levels(t0 + Duration::from_secs(1)), vec![0; BANDS]);
        assert_eq!(Spectrum::default().levels(t0), vec![0; BANDS]);
    }
}
