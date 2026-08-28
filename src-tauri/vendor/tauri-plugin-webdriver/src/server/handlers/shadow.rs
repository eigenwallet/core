use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use tauri::Runtime;

use crate::server::handlers::element::FindElementRequest;
use crate::server::response::{WebDriverErrorResponse, WebDriverResponse, WebDriverResult};
use crate::server::AppState;
use crate::webdriver::locator::LocatorStrategy;

/// GET `/session/{session_id}/element/{element_id}/shadow` - Get shadow root
pub async fn get_shadow_root<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path((session_id, element_id)): Path<(String, String)>,
) -> WebDriverResult {
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)?;

    let element = session
        .elements
        .get(&element_id)
        .ok_or_else(WebDriverErrorResponse::no_such_element)?;
    let element_js_var = element.js_ref.clone();

    // Create a new element ref for the shadow root
    let shadow_ref = session.elements.store();
    let shadow_js_var = shadow_ref.js_ref.clone();
    let shadow_id = shadow_ref.id.clone();
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let found = executor
        .get_element_shadow_root(&element_js_var, &shadow_js_var)
        .await?;

    if !found {
        return Err(WebDriverErrorResponse::no_such_shadow_root());
    }

    Ok(WebDriverResponse::success(json!({
        "shadow-6066-11e4-a52e-4f735466cecf": shadow_id
    })))
}

/// POST `/session/{session_id}/shadow/{shadow_id}/element` - Find element in shadow root
pub async fn find_element_in_shadow<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path((session_id, shadow_id)): Path<(String, String)>,
    Json(request): Json<FindElementRequest>,
) -> WebDriverResult {
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)?;

    // Shadow roots are stored in the same element store
    let shadow_element = session
        .elements
        .get(&shadow_id)
        .ok_or_else(WebDriverErrorResponse::no_such_shadow_root)?;
    let shadow_js_var = shadow_element.js_ref.clone();

    let strategy = LocatorStrategy::from_string(&request.using).ok_or_else(|| {
        WebDriverErrorResponse::invalid_argument(&format!(
            "Unknown locator strategy: {}",
            request.using
        ))
    })?;

    let element_ref = session.elements.store();
    let js_var = element_ref.js_ref.clone();
    let element_id = element_ref.id.clone();
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    // Use the locator method that generates expressions expecting `shadow` to be defined
    let strategy_js = strategy.to_selector_js_single_from_shadow(&request.value);

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let found = executor
        .find_element_from_shadow(&shadow_js_var, &strategy_js, &js_var)
        .await?;

    if !found {
        return Err(WebDriverErrorResponse::no_such_element());
    }

    Ok(WebDriverResponse::success(json!({
        "element-6066-11e4-a52e-4f735466cecf": element_id
    })))
}

/// POST `/session/{session_id}/shadow/{shadow_id}/elements` - Find elements in shadow root
pub async fn find_elements_in_shadow<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path((session_id, shadow_id)): Path<(String, String)>,
    Json(request): Json<FindElementRequest>,
) -> WebDriverResult {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)?;

    let shadow_element = session
        .elements
        .get(&shadow_id)
        .ok_or_else(WebDriverErrorResponse::no_such_shadow_root)?;
    let shadow_js_var = shadow_element.js_ref.clone();
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    let strategy = LocatorStrategy::from_string(&request.using).ok_or_else(|| {
        WebDriverErrorResponse::invalid_argument(&format!(
            "Unknown locator strategy: {}",
            request.using
        ))
    })?;

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let strategy_js = strategy.to_selector_js_from_shadow(&request.value);

    // Use a temporary prefix for the trait method
    let temp_prefix = "__wd_temp_";
    let count = executor
        .find_elements_from_shadow(&shadow_js_var, &strategy_js, temp_prefix)
        .await?;

    // Now store each element with proper references
    let mut elements = Vec::new();
    let mut sessions = state.sessions.write().await;
    let session = sessions.get_mut(&session_id)?;

    for i in 0..count {
        let element_ref = session.elements.store();
        let js_var = element_ref.js_ref.clone();
        let element_id = element_ref.id.clone();

        // Copy from temp storage to element's js_ref
        let copy_script = format!(
            "(function() {{ window.{js_var} = window['{temp_prefix}{i}'];  return true; }})()"
        );
        let _ = executor.evaluate_js(&copy_script).await;

        elements.push(json!({
            "element-6066-11e4-a52e-4f735466cecf": element_id
        }));
    }

    Ok(WebDriverResponse::success(elements))
}
