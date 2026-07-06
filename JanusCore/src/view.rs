//! View layer: helpers to format JSON responses consistently.

use serde_json;
use warp::{Reply, http::StatusCode};

/// Helper for generating consistent JSON responses.
pub struct PlayerView;

impl PlayerView {
    /// Raw JSON response for any serializable payload.
    pub fn json_response<T: serde::Serialize>(data: &T) -> impl Reply {
        warp::reply::json(data)
    }

    /// Standard success response carrying a `message` field.
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

    /// Standard error response with a custom HTTP status code.
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

    /// JSON response for volume-related operations.
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

    /// JSON response with the current music name.
    pub fn current_music_response(music_name: &str) -> impl Reply {
        let response = serde_json::json!({ "current_music": music_name });
        warp::reply::json(&response)
    }

    /// JSON response used when no track is currently playing.
    pub fn no_music_response() -> impl Reply {
        let response = serde_json::json!({ "message": "Aucune musique en cours de lecture." });
        warp::reply::json(&response)
    }

    /// JSON response containing the list of folders.
    pub fn folders_response(folders: &[String]) -> impl Reply {
        let response = serde_json::json!({ "folders": folders });
        warp::reply::json(&response)
    }

    
} 