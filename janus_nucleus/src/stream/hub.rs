//! Diffusion un-vers-N des octets encodés vers les auditeurs HTTP.

use bytes::Bytes;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::broadcast;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::Stream;

/// Nombre de chunks conservés pour les abonnés en retard.
///
/// À 192 ms par chunk, 128 chunks font environ 24 s de marge : de quoi absorber
/// un client qui bloque un instant sans le faire décrocher.
pub const DEFAULT_CAPACITY: usize = 128;

/// Distribue les chunks encodés à tous les auditeurs connectés.
///
/// Les chunks sont des [`Bytes`], donc partagés par comptage de références :
/// diffuser à N auditeurs ne copie pas les données N fois.
pub struct StreamHub {
    tx: broadcast::Sender<Bytes>,
    listeners: Arc<AtomicUsize>,
}

impl StreamHub {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self {
            tx,
            listeners: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Publie un chunk.
    ///
    /// Ne renvoie pas d'erreur quand personne n'écoute : le moteur doit continuer
    /// à tourner sans auditeur, sinon le premier client à se connecter tomberait
    /// sur un flux arrêté.
    pub fn publish(&self, chunk: Bytes) {
        let _ = self.tx.send(chunk);
    }

    /// Ouvre un abonnement. L'auditeur est décompté à la destruction du retour.
    pub fn subscribe(&self) -> Subscription {
        self.listeners.fetch_add(1, Ordering::Relaxed);
        Subscription {
            inner: BroadcastStream::new(self.tx.subscribe()),
            listeners: Arc::clone(&self.listeners),
        }
    }

    /// Nombre d'auditeurs actuellement connectés.
    pub fn listener_count(&self) -> usize {
        self.listeners.load(Ordering::Relaxed)
    }
}

impl Default for StreamHub {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

/// Flux d'octets MP3 pour un auditeur.
///
/// Implémente [`Stream`] plutôt que d'exposer le `Receiver` brut : c'est ce qui
/// garantit que le compteur d'auditeurs suit exactement la durée de vie de la
/// réponse HTTP, y compris quand le client coupe brutalement.
pub struct Subscription {
    inner: BroadcastStream<Bytes>,
    listeners: Arc<AtomicUsize>,
}

impl Stream for Subscription {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // `BroadcastStream` et `Arc` sont `Unpin`, donc `Subscription` l'est aussi.
        let this = self.get_mut();
        loop {
            return match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => Poll::Ready(Some(chunk)),
                // Client trop lent : les chunks manqués sont perdus, pas la
                // connexion. Le décodeur MP3 se recale sur la trame suivante, donc
                // l'auditeur entend un saut plutôt qu'une coupure.
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(_)))) => continue,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.listeners.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn diffuse_a_plusieurs_auditeurs() {
        let hub = StreamHub::new(8);
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();

        hub.publish(Bytes::from_static(b"trame"));

        assert_eq!(a.next().await, Some(Bytes::from_static(b"trame")));
        assert_eq!(b.next().await, Some(Bytes::from_static(b"trame")));
    }

    #[tokio::test]
    async fn compte_les_auditeurs_et_les_decompte_a_la_deconnexion() {
        let hub = StreamHub::new(8);
        assert_eq!(hub.listener_count(), 0);

        let a = hub.subscribe();
        let b = hub.subscribe();
        assert_eq!(hub.listener_count(), 2);

        drop(a);
        assert_eq!(hub.listener_count(), 1);
        drop(b);
        assert_eq!(hub.listener_count(), 0);
    }

    /// Un abonné qui laisse déborder le tampon doit sauter les chunks perdus et
    /// continuer à recevoir, et non voir son flux se terminer.
    #[tokio::test]
    async fn un_abonne_en_retard_saute_et_continue() {
        let hub = StreamHub::new(2);
        let mut lent = hub.subscribe();

        for i in 0u8..6 {
            hub.publish(Bytes::from(vec![i]));
        }

        // Le tampon ne garde que les deux derniers.
        assert_eq!(lent.next().await, Some(Bytes::from(vec![4u8])));
        assert_eq!(lent.next().await, Some(Bytes::from(vec![5u8])));
    }

    /// Publier sans auditeur ne doit pas être une erreur : le moteur tourne en
    /// continu, y compris quand la radio n'a personne à l'écoute.
    #[tokio::test]
    async fn publier_sans_auditeur_est_sans_effet() {
        let hub = StreamHub::new(4);
        hub.publish(Bytes::from_static(b"dans le vide"));
        assert_eq!(hub.listener_count(), 0);

        // Un auditeur qui arrive ensuite reçoit bien la suite.
        let mut tardif = hub.subscribe();
        hub.publish(Bytes::from_static(b"entendu"));
        assert_eq!(tardif.next().await, Some(Bytes::from_static(b"entendu")));
    }
}
