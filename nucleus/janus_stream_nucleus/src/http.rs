//! Réponse HTTP d'un flux audio continu.

use bytes::Bytes;
use std::convert::Infallible;
use tokio_stream::StreamExt;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;

use crate::stream::StreamHub;

/// Construit la réponse d'un flux sans fin.
///
/// Mutualisée parce que les en-têtes sont le point sensible, et qu'ils ne
/// pardonnent pas l'à-peu-près :
///
/// - **aucun `Content-Length`** — c'est son absence qui fait basculer hyper en
///   `Transfer-Encoding: chunked`, seul mode viable pour un flux sans fin. En
///   ajouter un, même « pour bien faire », casse la diffusion ;
/// - **`Accept-Ranges: none`** — une balise `<audio>` envoie volontiers
///   `Range: bytes=0-` ; on l'ignore et on répond 200, autant l'annoncer ;
/// - **`no-store`** — un intermédiaire qui mettrait le flux en cache servirait
///   éternellement les mêmes secondes.
///
/// L'abonnement porte le comptage des auditeurs : il vit exactement aussi
/// longtemps que le corps de la réponse, y compris si le client coupe net.
pub fn stream_response(hub: &StreamHub, station: &str, genre: Option<&str>) -> Response<Body> {
    let chunks = hub.subscribe().map(Ok::<Bytes, Infallible>);

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "audio/mpeg")
        .header("Cache-Control", "no-store, no-cache, must-revalidate")
        .header("Pragma", "no-cache")
        .header("Accept-Ranges", "none")
        .header("icy-name", station);

    if let Some(genre) = genre {
        builder = builder.header("icy-genre", genre);
    }

    builder
        .body(Body::wrap_stream(chunks))
        .expect("réponse de flux mal formée")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn les_en_tetes_du_flux_sont_corrects() {
        let hub = StreamHub::default();
        let r = stream_response(&hub, "TestCore", Some("Synthwave"));

        assert_eq!(r.status(), StatusCode::OK);
        let h = r.headers();
        assert_eq!(h.get("content-type").unwrap(), "audio/mpeg");
        assert_eq!(h.get("accept-ranges").unwrap(), "none");
        assert_eq!(h.get("icy-name").unwrap(), "TestCore");
        assert_eq!(h.get("icy-genre").unwrap(), "Synthwave");
        assert!(h.get("cache-control").unwrap().to_str().unwrap().contains("no-store"));

        // Le point qui casse tout s'il est oublié : sans Content-Length, hyper passe
        // en chunked, ce qu'exige un flux sans fin.
        assert!(
            h.get("content-length").is_none(),
            "un Content-Length rendrait le flux fini"
        );
    }

    #[tokio::test]
    async fn le_genre_est_facultatif() {
        let hub = StreamHub::default();
        let r = stream_response(&hub, "TestCore", None);
        assert!(r.headers().get("icy-genre").is_none());
    }

    #[tokio::test]
    async fn la_reponse_compte_un_auditeur_et_le_libere() {
        let hub = StreamHub::default();
        assert_eq!(hub.listener_count(), 0);

        let r = stream_response(&hub, "TestCore", None);
        assert_eq!(hub.listener_count(), 1, "auditeur non compté");

        drop(r);
        assert_eq!(hub.listener_count(), 0, "auditeur non libéré à la déconnexion");
    }
}
