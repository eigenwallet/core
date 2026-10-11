use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tauri::Runtime;

use crate::server::response::{WebDriverResponse, WebDriverResult};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct TimeoutsRequest {
    #[serde(default)]
    pub implicit: Option<u64>,
    #[serde(rename = "pageLoad", default)]
    pub page_load: Option<u64>,
    #[serde(default)]
    pub script: Option<u64>,
}

/// GET `/session/{session_id}/timeouts` - Get session timeouts
pub async fn get<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
) -> WebDriverResult {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)?;

    Ok(WebDriverResponse::success(json!({
        "implicit": session.timeouts.implicit_ms,
        "pageLoad": session.timeouts.page_load_ms,
        "script": session.timeouts.script_ms
    })))
}

/// POST `/session/{session_id}/timeouts` - Set session timeouts
pub async fn set<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
    Json(request): Json<TimeoutsRequest>,
) -> WebDriverResult {
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)?;

    if let Some(implicit) = request.implicit {
        session.timeouts.implicit_ms = implicit;
    }
    if let Some(page_load) = request.page_load {
        session.timeouts.page_load_ms = page_load;
    }
    if let Some(script) = request.script {
        session.timeouts.script_ms = script;
    }

    Ok(WebDriverResponse::null())
}
