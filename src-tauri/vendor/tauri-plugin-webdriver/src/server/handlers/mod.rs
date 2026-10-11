use std::sync::Arc;

use axum::extract::State;
use serde_json::json;
use tauri::Runtime;

use super::response::{WebDriverResponse, WebDriverResult};
use super::AppState;

pub mod actions;
pub mod alert;
pub mod cookie;
pub mod document;
pub mod element;
pub mod frame;
pub mod navigation;
pub mod print;
pub mod screenshot;
pub mod script;
pub mod session;
pub mod shadow;
pub mod timeouts;
pub mod window;

/// GET `/status` - `WebDriver` server status
pub async fn status<R: Runtime>(_state: State<Arc<AppState<R>>>) -> WebDriverResult {
    Ok(WebDriverResponse::success(json!({
        "ready": true,
        "message": "tauri-plugin-webdriver is ready"
    })))
}
