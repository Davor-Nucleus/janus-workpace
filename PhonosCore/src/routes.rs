use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use warp::{Filter, Rejection, Reply};

use crate::model::PlayerState;
use crate::controller::PlayerController;

pub fn create_routes(
    player: Arc<Mutex<PlayerState>>,
    music_port: u16,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let player_filter = warp::any().map(move || player.clone());

    // Route GET /api/soundboard/play?sound=nom_du_son
    let soundboard_route = warp::path!("api" / "soundboard" / "play")
        .and(warp::get())
        .and(warp::query::<HashMap<String, String>>())
        .and(player_filter.clone())
        .and(warp::any().map(move || music_port))
        .and_then(|params: HashMap<String, String>, player: Arc<Mutex<PlayerState>>, music_port: u16| async move {
            PlayerController::handle_soundboard_play(params, player, music_port).await
        });

    // Route GET /api/soundboard/sounds pour lister les sons disponibles
    let soundboard_sounds_route = warp::path!("api" / "soundboard" / "sounds")
        .and(warp::get())
        .and_then(PlayerController::handle_soundboard_sounds);

    // Route GET /api/soundboard/stop pour arrêter tous les sons de la soundboard
    let soundboard_stop_route = warp::path!("api" / "soundboard" / "stop")
        .and(warp::get())
        .and(player_filter.clone())
        .and(warp::any().map(move || music_port))
        .and_then(|player: Arc<Mutex<PlayerState>>, music_port: u16| async move {
            PlayerController::handle_soundboard_stop(player, music_port).await
        });

    // Combine toutes les routes (soundboard uniquement)
    soundboard_route
        .or(soundboard_sounds_route)
        .or(soundboard_stop_route)
}