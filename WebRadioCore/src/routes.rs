//! Routes Warp exposées par WebRadioCore.

use std::collections::HashMap;
use std::sync::Arc;
use warp::{Filter, Rejection, Reply};

use crate::controller::{RadioContext, RadioController};

pub fn create_routes(
    ctx: Arc<RadioContext>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let ctx_filter = warp::any().map(move || Arc::clone(&ctx));

    // GET /stream.mp3 — l'extension est là pour les clients qui devinent le type
    // depuis l'URL plutôt que depuis l'en-tête Content-Type.
    let stream = warp::path!("stream.mp3")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_stream);

    let status = warp::path!("api" / "status")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_status);

    let folder = warp::path!("api" / "folder")
        .and(warp::get())
        .and(warp::query::<HashMap<String, String>>())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_folder);

    let folderlist = warp::path!("api" / "folderlist")
        .and(warp::get())
        .and_then(RadioController::handle_folderlist);

    let current_music = warp::path!("api" / "current_music")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_current_music);

    let current_music_ws = warp::path!("api" / "current_music_ws")
        .and(warp::ws())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_current_music_ws);

    let next = warp::path!("api" / "next")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_next);

    let previous = warp::path!("api" / "previous")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_previous);

    let has_next = warp::path!("api" / "has_next")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_has_next);

    let has_previous = warp::path!("api" / "has_previous")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_has_previous);

    let pause = warp::path!("api" / "pause")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_pause);

    let resume = warp::path!("api" / "resume")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_resume);

    let get_volume = warp::path!("api" / "volume")
        .and(warp::get())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_get_volume);

    let set_volume = warp::path!("api" / "volume")
        .and(warp::post())
        .and(warp::body::json())
        .and(ctx_filter.clone())
        .and_then(RadioController::handle_set_volume);

    stream
        .or(status)
        .or(folder)
        .or(folderlist)
        .or(current_music)
        .or(current_music_ws)
        .or(next)
        .or(previous)
        .or(has_next)
        .or(has_previous)
        .or(pause)
        .or(resume)
        .or(get_volume)
        .or(set_volume)
}
