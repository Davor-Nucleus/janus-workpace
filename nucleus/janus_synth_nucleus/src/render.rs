//! Assemblage complet : arrangeur, instruments, mixage, effets.
//!
//! Isolé de la boucle temps réel pour rester testable hors de tout réseau et de
//! tout encodeur : on peut lui demander de rendre N secondes aussi vite que la
//! machine le permet, et vérifier le résultat.

use crate::compose::arranger::{Arranger, Event, Snapshot};
use crate::synth::drums::DrumMachine;
use crate::synth::fx::{soft_clip, PingPongDelay, Reverb, Sidechain};
use crate::synth::voice::{Instrument, ARP, BASS, LEAD, PAD};

/// Niveaux du mixage. Réglés pour que la somme reste sous le limiteur en régime
/// dense, sans avoir à compter sur l'écrêtage pour tenir le niveau.
const PAD_LEVEL: f32 = 0.40;
const BASS_LEVEL: f32 = 0.80;
const ARP_LEVEL: f32 = 0.28;
const LEAD_LEVEL: f32 = 0.32;
const DRUM_LEVEL: f32 = 0.90;

/// Profondeur et durée du ducking déclenché par la grosse caisse.
const SIDECHAIN_AMOUNT: f32 = 0.55;
const SIDECHAIN_RELEASE: f32 = 0.22;

pub struct Synth {
    sample_rate: f32,
    arranger: Arranger,

    pad: Instrument,
    bass: Instrument,
    arp: Instrument,
    lead: Instrument,
    drums: DrumMachine,

    sidechain: Sidechain,
    delay: PingPongDelay,
    reverb: Reverb,
}

impl Synth {
    pub fn new(sample_rate: f32, bpm: f32, seed: u64) -> Self {
        let arranger = Arranger::new(sample_rate, bpm, seed);
        let delay = Self::build_delay(sample_rate, arranger.bpm());

        Self {
            sample_rate,
            arranger,
            // Quatre voix pour la nappe : de quoi tenir un accord de septième.
            pad: Instrument::new(sample_rate, PAD, 4),
            bass: Instrument::new(sample_rate, BASS, 1),
            // Deux voix pour l'arpège : les notes pincées se chevauchent.
            arp: Instrument::new(sample_rate, ARP, 2),
            lead: Instrument::new(sample_rate, LEAD, 1),
            drums: DrumMachine::new(sample_rate, seed),
            sidechain: Sidechain::new(sample_rate, SIDECHAIN_RELEASE, SIDECHAIN_AMOUNT),
            delay,
            reverb: Reverb::new(sample_rate, 0.35),
        }
    }

    /// Delay calé sur la croche pointée, l'intervalle qui donne son balancement au genre.
    fn build_delay(sample_rate: f32, bpm: f32) -> PingPongDelay {
        let dotted_eighth = 0.75 * 60.0 / bpm;
        PingPongDelay::new(sample_rate, dotted_eighth, 0.42, 0.45)
    }

    /// Repart sur une nouvelle graine, sans interrompre la production.
    ///
    /// Le delay est reconstruit parce que sa longueur dépend du tempo. L'allocation
    /// se fait dans le fil audio, mais elle ne coûte que quelques dizaines de
    /// microsecondes face aux 192 ms d'un bloc, et ne survient que sur action
    /// explicite.
    pub fn regenerate(&mut self, seed: u64, bpm: f32) {
        self.arranger.regenerate(seed, bpm);
        self.delay = Self::build_delay(self.sample_rate, self.arranger.bpm());
        self.pad.all_notes_off();
        self.bass.all_notes_off();
        self.arp.all_notes_off();
        self.lead.all_notes_off();
    }

    pub fn snapshot(&self) -> Snapshot {
        self.arranger.snapshot()
    }

    fn dispatch(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::PadChord(tones) => {
                    self.pad.all_notes_off();
                    for note in tones {
                        self.pad.note_on(note as f32);
                    }
                }
                Event::PadOff => self.pad.all_notes_off(),
                Event::Bass(note) => self.bass.note_on(note as f32),
                Event::Arp(note) => self.arp.note_on(note as f32),
                Event::Lead(note) => self.lead.note_on(note as f32),
                Event::Percussion(drum) => self.drums.trigger(drum),
            }
        }
    }

    /// Rend un bloc stéréo entrelacé. `out` doit avoir une longueur paire.
    pub fn render(&mut self, out: &mut [i16], volume: f32) {
        let volume = volume.clamp(0.0, 1.0);

        for frame in out.chunks_exact_mut(2) {
            // Les événements sont déclenchés échantillon par échantillon : les
            // grouper par bloc décalerait la batterie de près de 200 ms, ce qui
            // s'entend immédiatement comme un défaut de mise en place.
            if let Some(events) = self.arranger.advance() {
                self.dispatch(events);
            }
            if self.drums.take_kick_trigger() {
                self.sidechain.trigger();
            }

            let cutoff = self.arranger.cutoff_scale();
            let pad_dry = self.pad.next(cutoff);
            let bass_dry = self.bass.next(cutoff);
            let arp_dry = self.arp.next(cutoff);
            let lead_dry = self.lead.next(cutoff);
            let drums_dry = self.drums.next();

            // La batterie n'est pas ducked : c'est elle qui déclenche le ducking.
            let duck = self.sidechain.next_gain();
            let pad = pad_dry * PAD_LEVEL * duck;
            let bass = bass_dry * BASS_LEVEL * duck;
            let arp = arp_dry * ARP_LEVEL * duck;
            let lead = lead_dry * LEAD_LEVEL * duck;
            let drums = drums_dry * DRUM_LEVEL;

            // Arpège et lead traversent le delay ; l'arpège est légèrement décalé à
            // droite pour que le ping-pong ait de quoi travailler.
            let (echo_l, echo_r) = self.delay.process(arp * 0.75 + lead, arp * 1.25 + lead);
            // La nappe passe par la réverbération, qui lui donne sa largeur.
            let (space_l, space_r) = self.reverb.process(pad, pad);

            let l = soft_clip((echo_l + space_l + bass + drums) * volume);
            let r = soft_clip((echo_r + space_r + bass + drums) * volume);

            frame[0] = (l * i16::MAX as f32) as i16;
            frame[1] = (r * i16::MAX as f32) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use janus_stream_nucleus::{CHUNK_SAMPLES, OUTPUT_SAMPLE_RATE};
    use std::time::Instant;

    const FS: f32 = OUTPUT_SAMPLE_RATE as f32;

    fn rendre(seed: u64, secondes: f32) -> Vec<i16> {
        let mut synth = Synth::new(FS, 92.0, seed);
        let frames = (FS * secondes) as usize;
        let mut out = vec![0i16; frames * 2];
        // Par blocs, comme le moteur, pour éprouver le même chemin de code.
        for bloc in out.chunks_mut(CHUNK_SAMPLES) {
            synth.render(bloc, 1.0);
        }
        out
    }

    #[test]
    fn le_rendu_produit_du_son() {
        let pcm = rendre(1, 4.0);
        let energie: i64 = pcm.iter().map(|s| (*s as i64).abs()).sum();
        assert!(energie > 0, "silence complet");

        let crete = pcm.iter().map(|s| s.abs() as i32).max().unwrap_or(0);
        assert!(crete > 3_000, "niveau anormalement bas : crête {crete}");
    }

    /// Le limiteur doit tenir : aucun échantillon ne doit atteindre la butée, sans
    /// quoi le flux crépiterait sur chaque pointe.
    #[test]
    fn le_niveau_ne_sature_jamais() {
        for seed in [1u64, 77, 4242] {
            let pcm = rendre(seed, 6.0);
            let crete = pcm.iter().map(|s| s.abs() as i32).max().unwrap_or(0);
            assert!(
                crete < i16::MAX as i32,
                "graine {seed} : saturation à {crete}"
            );
        }
    }

    /// Un `NaN` entré dans le delay ou la réverbération n'en ressortirait jamais :
    /// le flux resterait cassé jusqu'au redémarrage. On rend longtemps et à toutes
    /// les énergies pour s'en assurer.
    #[test]
    fn aucun_echantillon_degenere_sur_la_duree() {
        let mut synth = Synth::new(FS, 92.0, 31);
        let mut bloc = vec![0i16; CHUNK_SAMPLES];
        // ~60 s : plusieurs sections, donc toutes les énergies traversées.
        for _ in 0..(FS as usize * 60 / (CHUNK_SAMPLES / 2)) {
            synth.render(&mut bloc, 1.0);
            // Un NaN converti en i16 donne 0 : on surveille plutôt la butée, qui
            // signalerait une divergence en amont de l'écrêtage.
            assert!(
                bloc.iter().all(|s| *s > i16::MIN && *s < i16::MAX),
                "échantillon en butée : divergence en amont du limiteur"
            );
        }
    }

    #[test]
    fn la_meme_graine_rend_exactement_le_meme_audio() {
        assert_eq!(rendre(42, 2.0), rendre(42, 2.0));
    }

    #[test]
    fn deux_graines_rendent_un_audio_different() {
        assert_ne!(rendre(1, 2.0), rendre(2, 2.0));
    }

    #[test]
    fn le_volume_agit_sur_le_niveau() {
        let fort = {
            let mut s = Synth::new(FS, 92.0, 5);
            let mut b = vec![0i16; CHUNK_SAMPLES * 20];
            s.render(&mut b, 1.0);
            b.iter().map(|x| (*x as i64).abs()).sum::<i64>()
        };
        let faible = {
            let mut s = Synth::new(FS, 92.0, 5);
            let mut b = vec![0i16; CHUNK_SAMPLES * 20];
            s.render(&mut b, 0.25);
            b.iter().map(|x| (*x as i64).abs()).sum::<i64>()
        };
        assert!(faible < fort / 2, "le volume n'atténue pas : {faible} vs {fort}");
    }

    #[test]
    fn le_volume_nul_donne_le_silence() {
        let mut s = Synth::new(FS, 92.0, 6);
        let mut b = vec![0i16; CHUNK_SAMPLES * 10];
        s.render(&mut b, 0.0);
        assert!(b.iter().all(|x| *x == 0), "du son sort à volume nul");
    }

    #[test]
    fn la_regeneration_change_la_musique_sans_casser_le_rendu() {
        let mut synth = Synth::new(FS, 92.0, 10);
        let mut avant = vec![0i16; CHUNK_SAMPLES * 10];
        synth.render(&mut avant, 1.0);

        synth.regenerate(999, 92.0);
        let mut apres = vec![0i16; CHUNK_SAMPLES * 10];
        synth.render(&mut apres, 1.0);

        assert_ne!(avant, apres, "la régénération n'a rien changé");
        assert!(
            apres.iter().all(|s| *s > i16::MIN && *s < i16::MAX),
            "le rendu est cassé après régénération"
        );
        let energie: i64 = apres.iter().map(|s| (*s as i64).abs()).sum();
        assert!(energie > 0, "silence après régénération");
    }

    /// Le niveau doit rester dans une plage étroite sur la durée.
    ///
    /// C'est le test qui traduit en dB ce qu'une marche d'énergie mal réglée
    /// produit : mesuré sur sept minutes de flux réel, l'ancienne version tombait à
    /// −20 dB pendant des minutes entières, ce qui s'entend comme un décrochage et
    /// non comme une nuance.
    #[test]
    fn le_niveau_reste_stable_sur_la_duree() {
        let pcm = rendre(23, 180.0);
        let fenetre = (FS * 20.0) as usize * 2; // tranches de 20 s, stéréo

        let niveaux: Vec<f32> = pcm
            .chunks_exact(fenetre)
            .map(|tranche| {
                let somme: f64 = tranche.iter().map(|s| (*s as f64 / 32768.0).powi(2)).sum();
                let rms = (somme / tranche.len() as f64).sqrt().max(1e-9);
                20.0 * (rms as f32).log10()
            })
            .collect();

        let mini = niveaux.iter().cloned().fold(f32::MAX, f32::min);
        let maxi = niveaux.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            maxi - mini < 9.0,
            "amplitude de {:.1} dB entre tranches : {niveaux:?}",
            maxi - mini
        );
        assert!(mini > -30.0, "tranche quasi muette à {mini:.1} dB");
    }

    /// Le garde-fou contre une régression de performance. En production, un rendu
    /// plus lent que le temps réel ne se verrait pas : il s'entendrait, comme un
    /// hachage du flux pour tous les auditeurs.
    #[test]
    fn le_rendu_tient_largement_le_temps_reel() {
        let secondes = 10.0;
        let debut = Instant::now();
        let _ = rendre(3, secondes);
        let ecoule = debut.elapsed().as_secs_f32();

        assert!(
            ecoule < secondes / 3.0,
            "{secondes} s d'audio rendues en {ecoule:.2} s — marge insuffisante"
        );
    }
}
