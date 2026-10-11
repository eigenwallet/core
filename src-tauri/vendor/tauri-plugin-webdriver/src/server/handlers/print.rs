use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use tauri::Runtime;

use crate::platform::PrintOptions;
use crate::server::response::{WebDriverResponse, WebDriverResult};
use crate::server::AppState;

#[derive(Debug, Default, Deserialize)]
pub struct PrintRequest {
    #[serde(default)]
    pub orientation: Option<String>,
    #[serde(default)]
    pub scale: Option<f64>,
    #[serde(default)]
    pub background: Option<bool>,
    #[serde(default, rename = "pageWidth")]
    pub page_width: Option<f64>,
    #[serde(default, rename = "pageHeight")]
    pub page_height: Option<f64>,
    #[serde(default, rename = "marginTop")]
    pub margin_top: Option<f64>,
    #[serde(default, rename = "marginBottom")]
    pub margin_bottom: Option<f64>,
    #[serde(default, rename = "marginLeft")]
    pub margin_left: Option<f64>,
    #[serde(default, rename = "marginRight")]
    pub margin_right: Option<f64>,
    #[serde(default, rename = "shrinkToFit")]
    pub shrink_to_fit: Option<bool>,
    #[serde(default, rename = "pageRanges")]
    pub page_ranges: Option<Vec<String>>,
}

impl From<PrintRequest> for PrintOptions {
    fn from(req: PrintRequest) -> Self {
        PrintOptions {
            orientation: req.orientation,
            scale: req.scale,
            background: req.background,
            page_width: req.page_width,
            page_height: req.page_height,
            margin_top: req.margin_top,
            margin_bottom: req.margin_bottom,
            margin_left: req.margin_left,
            margin_right: req.margin_right,
            shrink_to_fit: req.shrink_to_fit,
            page_ranges: req.page_ranges,
        }
    }
}

/// POST `/session/{session_id}/print` - Print page to PDF
pub async fn print<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
    Json(request): Json<PrintRequest>,
) -> WebDriverResult {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&session_id)?;
    let current_window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frame_context = session.frame_context.clone();
    drop(sessions);

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let pdf_base64 = executor.print_page(request.into()).await?;

    Ok(WebDriverResponse::success(pdf_base64))
}
