//! Routes Warp exposées par OrpheusCore.

use std::sync::Arc;
use warp::{Filter, Rejection, Reply};

use crate::controller::{OrpheusContext, OrpheusController};

pub fn create_routes(
    ctx: Arc<OrpheusContext>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let ctx_filter = warp::any().map(move || Arc::clone(&ctx));

    let stream = warp::path!("stream.mp3")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_stream);

    let status = warp::path!("api" / "status")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_status);

    let get_volume = warp::path!("api" / "volume")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_get_volume);

    let set_volume = warp::path!("api" / "volume")
        .and(warp::post())
        .and(warp::body::json())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_set_volume);

    let volume_subtract = warp::path!("api" / "volume" / "subtract")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_volume_subtract);

    let volume_add = warp::path!("api" / "volume" / "add")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_volume_add);

    let regenerate = warp::path!("api" / "regenerate")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(OrpheusController::handle_regenerate);

    stream
        .or(status)
        .or(get_volume)
        .or(set_volume)
        .or(volume_subtract)
        .or(volume_add)
        .or(regenerate)
}
