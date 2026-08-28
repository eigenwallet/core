use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use tauri::Runtime;

use crate::server::response::{WebDriverResponse, WebDriverResult};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct ExecuteScriptRequest {
    pub script: String,
    #[serde(default)]
    pub args: Vec<Value>,
}

/// POST `/session/{session_id}/execute/sync` - Execute synchronous script
pub async fn execute_sync<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
    Json(request): Json<ExecuteScriptRequest>,
) -> WebDriverResult {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)?;
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let result = executor
        .execute_script(&request.script, &request.args)
        .await?;
    Ok(WebDriverResponse::success(result))
}

/// POST `/session/{session_id}/execute/async` - Execute asynchronous script
pub async fn execute_async<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
    Json(request): Json<ExecuteScriptRequest>,
) -> WebDriverResult {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)?;
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let result = executor
        .execute_async_script(&request.script, &request.args)
        .await?;
    Ok(WebDriverResponse::success(result))
}
