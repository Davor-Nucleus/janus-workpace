//! Capacite « musique procedurale » : synthese continue, sans aucun fichier audio.
//!
//! Deux etages, volontairement separes : `compose` decide *quoi* jouer (theorie,
//! progression, arrangement), `synth` sait seulement *produire du son*
//! (oscillateurs, enveloppes, filtres, voix, batterie, effets). `render` assemble
//! les deux et rend des blocs de PCM entrelace.
//!
//! Aucune dependance vers le reseau ni vers l'encodeur : un enfant peut rendre N
//! secondes aussi vite que la machine le permet et verifier le resultat. Tout
//! l'alea passe par un unique generateur ensemence, donc une graine determine
//! entierement la musique produite.

pub mod compose;
pub mod render;
pub mod synth;
