use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use tauri::Runtime;

use crate::platform::FrameId;
use crate::server::response::{WebDriverErrorResponse, WebDriverResponse, WebDriverResult};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct SwitchFrameRequest {
    pub id: Value,
}

/// POST `/session/{session_id}/frame` - Switch to frame
pub async fn switch_to_frame<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
    Json(request): Json<SwitchFrameRequest>,
) -> WebDriverResult {
    // First, read session data without modifying
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)?;

    // Get current context for validation (before any changes)
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let current_frame_context = session.frame_context.clone();

    // Parse the frame ID to determine what we're switching to
    let (frame_id, js_var_for_element) = match &request.id {
        Value::Null => {
            // Switch to top-level context - no validation needed
            drop(sessions);

            // Update session: clear frame context
            let mut sessions = state.sessions.write().await;
            let session = sessions.get_mut(&session_id)?;
            session.frame_context.clear();

            return Ok(WebDriverResponse::null());
        }
        Value::Number(n) => {
            let index = n.as_u64().ok_or_else(|| {
                WebDriverErrorResponse::invalid_argument(
                    "Frame index must be a non-negative integer",
                )
            })?;
            let index = u32::try_from(index)
                .map_err(|_| WebDriverErrorResponse::invalid_argument("Frame index too large"))?;

            (FrameId::Index(index), None)
        }
        Value::Object(obj) => {
            // W3C element reference format
            if let Some(element_id) = obj.get("element-6066-11e4-a52e-4f735466cecf") {
                let element_id = element_id.as_str().ok_or_else(|| {
                    WebDriverErrorResponse::invalid_argument("Element reference must be a string")
                })?;

                // Look up the element's js_var
                let element = session
                    .elements
                    .get(element_id)
                    .ok_or_else(WebDriverErrorResponse::no_such_element)?;

                let js_var = element.js_ref.clone();
                (FrameId::Element(js_var.clone()), Some(js_var))
            } else {
                return Err(WebDriverErrorResponse::invalid_argument(
                    "Invalid frame identifier object",
                ));
            }
        }
        _ => {
            return Err(WebDriverErrorResponse::invalid_argument(
                "Frame ID must be null, a number, or an element reference",
            ));
        }
    };
    drop(sessions);

    // Create executor with CURRENT frame context (not the new one) to validate
    let executor =
        state.get_executor_for_window(&current_window, timeouts, current_frame_context)?;

    // Validate the frame exists from current context
    executor.switch_to_frame(frame_id.clone()).await?;

    // Validation passed - now update the session's frame context
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)?;

    match &frame_id {
        FrameId::Index(_) => {
            session.frame_context.push(frame_id);
        }
        FrameId::Element(_) => {
            if let Some(js_var) = js_var_for_element {
                session.frame_context.push(FrameId::Element(js_var));
            }
        }
    }

    Ok(WebDriverResponse::null())
}

/// POST `/session/{session_id}/frame/parent` - Switch to parent frame
pub async fn switch_to_parent_frame<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
) -> WebDriverResult {
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)?;

    // Pop one level from frame context
    session.frame_context.pop();

    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    executor.switch_to_parent_frame().await?;

    Ok(WebDriverResponse::null())
}
