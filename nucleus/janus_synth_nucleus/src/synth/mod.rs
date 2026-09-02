//! Synthèse sonore : oscillateurs, enveloppes, filtres, voix, batterie, effets.
//!
//! Tout est calculé en `f32` échantillon par échantillon. Deux propriétés sont
//! tenues à chaque étage, parce qu'un serveur qui diffuse en continu ne peut pas
//! se permettre de les perdre :
//!
//! - **rien ne diverge** : les filtres et les boucles de rétroaction sont bornés ;
//! - **rien ne propage un non-fini** : un `NaN` entré dans un delay ou une
//!   réverbération n'en ressort jamais, et le flux resterait cassé jusqu'au
//!   redémarrage. Chaque étage à mémoire se purge plutôt que de le laisser passer.

pub mod adsr;
pub mod drums;
pub mod filter;
pub mod fx;
pub mod osc;
pub mod voice;
