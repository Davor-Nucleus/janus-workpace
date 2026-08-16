//! État partagé entre les handlers HTTP et le moteur de rendu.

use janus_nucleus::config::persist_key;

use crate::compose::arranger::Snapshot;

pub struct OrpheusState {
    volume: f32,
    /// Dernier instantané publié par le moteur.
    ///
    /// Le moteur le dépose une fois par bloc ; les handlers le lisent. Sans cette
    /// copie, `/api/status` devrait interroger l'arrangeur, donc partager son
    /// verrou avec le fil de rendu.
    snapshot: Snapshot,
    /// Incrémenté par `/api/regenerate` ; le moteur compare et réagit.
    regeneration: u64,
    /// Graine à appliquer à la prochaine régénération.
    pending_seed: u64,
}

impl OrpheusState {
    pub fn new(volume: f32, snapshot: Snapshot, seed: u64) -> Self {
        Self {
            volume: volume.clamp(0.0, 1.0),
            snapshot,
            regeneration: 0,
            pending_seed: seed,
        }
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Règle le volume et le persiste sous `ORPHEUS_VOLUME`.
    ///
    /// Clé dédiée : `VOLUME` est déjà partagée par JanusCore et PhonosCore, et
    /// `WEBRADIO_VOLUME` par la webradio. Chaque serveur écrit la sienne.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        persist_key("ORPHEUS_VOLUME", serde_json::json!(self.volume));
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn publish_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot;
    }

    pub fn regeneration(&self) -> u64 {
        self.regeneration
    }

    pub fn pending_seed(&self) -> u64 {
        self.pending_seed
    }

    /// Demande au moteur de repartir sur une nouvelle graine.
    pub fn request_regeneration(&mut self, seed: u64) {
        self.pending_seed = seed;
        self.regeneration = self.regeneration.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instantane() -> Snapshot {
        Snapshot {
            bpm: 92.0,
            key: "La mineur".to_string(),
            chord: "i (La)".to_string(),
            bar: 0,
            section: 0,
            energy: 2,
            seed: 1,
        }
    }

    #[test]
    fn le_volume_est_borne_a_la_construction() {
        assert_eq!(OrpheusState::new(3.0, instantane(), 1).volume(), 1.0);
        assert_eq!(OrpheusState::new(-2.0, instantane(), 1).volume(), 0.0);
    }

    #[test]
    fn une_demande_de_regeneration_change_le_compteur_et_la_graine() {
        let mut s = OrpheusState::new(1.0, instantane(), 7);
        let avant = s.regeneration();

        s.request_regeneration(1234);
        assert_ne!(s.regeneration(), avant, "le moteur ne sera pas prévenu");
        assert_eq!(s.pending_seed(), 1234);
    }

    #[test]
    fn l_instantane_publie_est_celui_qu_on_relit() {
        let mut s = OrpheusState::new(1.0, instantane(), 1);
        let mut nouveau = instantane();
        nouveau.bar = 42;
        nouveau.energy = 3;

        s.publish_snapshot(nouveau);
        assert_eq!(s.snapshot().bar, 42);
        assert_eq!(s.snapshot().energy, 3);
    }
}
