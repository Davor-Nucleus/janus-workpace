use serde_json;
use warp::{Reply, http::StatusCode};

pub struct PlayerView;

impl PlayerView {
    pub fn json_response<T: serde::Serialize>(data: &T) -> impl Reply {
        warp::reply::json(data)
    }

    pub fn success_message(message: &str) -> impl Reply {
        let response = serde_json::json!({
            "status": "success",
            "message": message
        });
        warp::reply::with_status(
            warp::reply::json(&response),
            StatusCode::OK,
        )
    }

    pub fn error_message(message: &str, status: StatusCode) -> impl Reply {
        let response = serde_json::json!({
            "status": "error",
            "message": message
        });
        warp::reply::with_status(
            warp::reply::json(&response),
            status,
        )
    }

    pub fn volume_response(volume: f32, message: &str) -> impl Reply {
        let response = serde_json::json!({
            "message": message,
            "volume": volume
        });
        warp::reply::with_status(
            warp::reply::json(&response),
            StatusCode::OK,
        )
    }

    pub fn current_music_response(music_name: &str) -> impl Reply {
        let response = serde_json::json!({ "current_music": music_name });
        warp::reply::json(&response)
    }

    pub fn no_music_response() -> impl Reply {
        let response = serde_json::json!({ "message": "Aucune musique en cours de lecture." });
        warp::reply::json(&response)
    }

    pub fn folders_response(folders: &[String]) -> impl Reply {
        let response = serde_json::json!({ "folders": folders });
        warp::reply::json(&response)
    }

    pub fn soundboard_response(sound_name: &str) -> impl Reply {
        let response = serde_json::json!({
            "status": "success",
            "message": format!("Son {:?} joué", sound_name)
        });
        warp::reply::with_status(
            warp::reply::json(&response),
            StatusCode::OK,
        )
    }
} 