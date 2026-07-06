//! Definition of the Warp routes exposed by the server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use warp::{Filter, Rejection, Reply};

use crate::model::PlayerState;
use crate::controller::PlayerController;

/// Build and return the full set of Warp routes to be served.
pub fn create_routes(
    player: Arc<Mutex<PlayerState>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let player_filter = warp::any().map(move || player.clone());

    // Route GET /api/folder?folder=xxx
    let folder_route = warp::path!("api" / "folder")
        .and(warp::get())
        .and(warp::query::<HashMap<String, String>>())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_folder);

    // Route GET /api/stop
    let stop_route = warp::path!("api" / "stop")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_stop);

    // Route GET /api/pause
    let pause_route = warp::path!("api" / "pause")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_pause);

    // Route GET /api/folderlist
    let folderlist_route = warp::path!("api" / "folderlist")
        .and(warp::get())
        .and_then(PlayerController::handle_folderlist);

    // Route GET /api/volume
    let get_volume_route = warp::path!("api" / "volume")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_get_volume);

    // Route POST /api/volume
    let set_volume_route = warp::path!("api" / "volume")
        .and(warp::post())
        .and(warp::body::json())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_set_volume);

    // Route GET /api/volume/add
    let volume_add_route = warp::path!("api" / "volume" / "add")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_volume_add);

    // Route GET /api/volume/subtract
    let volume_subtract_route = warp::path!("api" / "volume" / "subtract")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_volume_subtract);

    // Route GET /api/current_music
    let current_music_route = warp::path!("api" / "current_music")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_current_music);


    // Route ws /api/current_music
    let current_music_ws_route = warp::path!("api" / "current_music_ws")
        .and(warp::ws())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_current_music_ws);

    // Routes soundboard supprimées

    // Route GET /api/previous pour jouer la piste précédente
    let previous_route = warp::path!("api" / "previous")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_previous);

    // Route GET /api/has_previous pour vérifier s'il y a une piste précédente
    let has_previous_route = warp::path!("api" / "has_previous")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_has_previous);

    // Route GET /api/next pour jouer la piste suivante
    let next_route = warp::path!("api" / "next")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_next);

    // Route GET /api/has_next pour vérifier s'il y a une piste suivante
    let has_next_route = warp::path!("api" / "has_next")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_has_next);

    // Route GET /api/limiter
    let get_limiter_route = warp::path!("api" / "limiter")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_get_limiter);

    // Route POST /api/limiter
    let set_limiter_route = warp::path!("api" / "limiter")
        .and(warp::post())
        .and(warp::body::json())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_set_limiter);

    // Route GET /api/limiter/add
    let limiter_add_route = warp::path!("api" / "limiter" / "add")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_limiter_add);

    // Route GET /api/limiter/subtract
    let limiter_subtract_route = warp::path!("api" / "limiter" / "subtract")
        .and(warp::get())
        .and(player_filter.clone())
        .and_then(PlayerController::handle_limiter_subtract);

    // Combine toutes les routes
    folder_route
        .or(stop_route)
        .or(pause_route)
        .or(folderlist_route)
        .or(get_volume_route)
        .or(set_volume_route)
        .or(volume_add_route)
        .or(volume_subtract_route)
        .or(current_music_route)
        .or(current_music_ws_route)
        .or(previous_route)
        .or(has_previous_route)
        .or(next_route)
        .or(has_next_route)
        .or(get_limiter_route)
        .or(set_limiter_route)
        .or(limiter_add_route)
        .or(limiter_subtract_route)
} 