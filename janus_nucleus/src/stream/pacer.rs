//! Cadenceur horloge du producteur audio.

use std::time::{Duration, Instant};

/// Ramène un producteur audio au temps réel.
///
/// Le producteur appelle [`Pacer::commit`] après chaque bloc d'échantillons ; le
/// pacer dort jusqu'à l'instant où ce bloc « aurait dû » être fini.
///
/// L'échéance est **absolue** (`origine + total_frames / fréquence`) et non
/// relative (`sleep(durée_du_bloc)`). C'est le point qui décide de la justesse du
/// flux : la granularité du timer Windows est d'environ 15 ms, donc chaque
/// `sleep` relatif dépasse un peu sa cible, et l'erreur s'additionne. À 192 ms par
/// bloc, un dépassement moyen de 5 ms représente déjà 1,5 minute de dérive par
/// jour. Avec une échéance absolue, un bloc en retard est simplement suivi d'une
/// attente plus courte, et l'erreur ne s'accumule jamais.
pub struct Pacer {
    sample_rate: u32,
    origin: Instant,
    frames_total: u64,
}

impl Pacer {
    /// `lead` est l'avance permanente que le producteur garde sur l'horloge.
    ///
    /// Elle se traduit par autant d'audio déjà encodé et distribué en avance, ce
    /// qui donne au client de quoi absorber la gigue du réseau sans couper. On la
    /// paie en latence, sans conséquence pour une radio.
    pub fn new(sample_rate: u32, lead: Duration) -> Self {
        Self {
            sample_rate,
            origin: Instant::now()
                .checked_sub(lead)
                .unwrap_or_else(Instant::now),
            frames_total: 0,
        }
    }

    /// Signale `frames` produites, puis dort jusqu'à leur échéance.
    ///
    /// Ne dort pas si l'échéance est déjà passée : le producteur rattrape alors
    /// son retard à pleine vitesse.
    pub fn commit(&mut self, frames: u64) {
        self.frames_total = self.frames_total.saturating_add(frames);
        if let Some(wait) = self.deadline().checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }

    /// Repart de l'instant courant, en conservant `lead`.
    ///
    /// À utiliser après une interruption longue (reprise de pause), faute de quoi
    /// le producteur se croirait très en retard et débiterait plusieurs minutes
    /// d'audio d'un coup.
    pub fn reset(&mut self, lead: Duration) {
        self.origin = Instant::now()
            .checked_sub(lead)
            .unwrap_or_else(Instant::now);
        self.frames_total = 0;
    }

    /// Audio produit depuis la dernière remise à zéro.
    pub fn elapsed_audio(&self) -> Duration {
        Self::frames_to_duration(self.frames_total, self.sample_rate)
    }

    fn deadline(&self) -> Instant {
        self.origin + Self::frames_to_duration(self.frames_total, self.sample_rate)
    }

    /// Passe par `u128` : à 48 kHz, `frames * 1_000_000_000` déborde un `u64` au
    /// bout de quatre jours et demi de flux, ce qu'une radio atteint sans peine.
    fn frames_to_duration(frames: u64, sample_rate: u32) -> Duration {
        let nanos = frames as u128 * 1_000_000_000u128 / sample_rate.max(1) as u128;
        Duration::from_nanos(nanos.min(u64::MAX as u128) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convertit_les_frames_en_duree() {
        assert_eq!(
            Pacer::frames_to_duration(48_000, 48_000),
            Duration::from_secs(1)
        );
        assert_eq!(
            Pacer::frames_to_duration(24_000, 48_000),
            Duration::from_millis(500)
        );
    }

    /// Le calcul doit rester juste au-delà du point où `frames * 1e9` déborderait
    /// un `u64` — soit environ 4,5 jours de flux à 48 kHz.
    #[test]
    fn ne_deborde_pas_sur_une_longue_diffusion() {
        let une_semaine = 48_000u64 * 60 * 60 * 24 * 7;
        let d = Pacer::frames_to_duration(une_semaine, 48_000);
        assert_eq!(d, Duration::from_secs(60 * 60 * 24 * 7));
    }

    /// Le temps réellement écoulé doit suivre le temps audio produit, sans dérive
    /// cumulée sur une série de blocs.
    #[test]
    fn cadence_au_temps_reel() {
        // 10 blocs de 4800 frames à 48 kHz = 1 s d'audio, sans avance initiale.
        let mut pacer = Pacer::new(48_000, Duration::ZERO);
        let depart = Instant::now();
        for _ in 0..10 {
            pacer.commit(4_800);
        }
        let ecoule = depart.elapsed();

        assert_eq!(pacer.elapsed_audio(), Duration::from_secs(1));
        // Borne basse stricte : le pacer ne doit jamais rendre la main en avance.
        assert!(ecoule >= Duration::from_millis(980), "trop rapide : {ecoule:?}");
        // Borne haute large : le timer Windows a ~15 ms de granularité par sleep,
        // mais l'échéance absolue empêche ces retards de s'additionner.
        assert!(ecoule < Duration::from_millis(1_200), "trop lent : {ecoule:?}");
    }

    /// Un bloc en retard doit être suivi d'une attente raccourcie, pas d'une
    /// attente pleine : c'est toute la différence avec un `sleep` relatif.
    #[test]
    fn rattrape_un_retard_sans_le_reporter() {
        let mut pacer = Pacer::new(48_000, Duration::ZERO);
        // Simule un bloc dont la production a pris trop de temps.
        std::thread::sleep(Duration::from_millis(300));

        let depart = Instant::now();
        pacer.commit(4_800); // 100 ms d'audio, échéance déjà dépassée
        pacer.commit(4_800); // 200 ms cumulées, toujours dépassée
        // Les deux commits ne doivent pas avoir attendu.
        assert!(depart.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn reset_repart_de_zero() {
        let mut pacer = Pacer::new(48_000, Duration::ZERO);
        pacer.commit(4_800);
        pacer.reset(Duration::ZERO);
        assert_eq!(pacer.elapsed_audio(), Duration::ZERO);
    }
}
