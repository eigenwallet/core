use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    Router,
};
use tauri::Runtime;

use super::handlers;
use super::AppState;

/// Create the `WebDriver` router with all W3C `WebDriver` endpoints
#[allow(clippy::too_many_lines)]
pub fn create_router<R: Runtime + 'static>(state: Arc<AppState<R>>) -> Router {
    Router::new()
        // Status
        .route("/status", get(handlers::status::<R>))
        // Session management
        .route("/session", post(handlers::session::create::<R>))
        .route(
            "/session/{session_id}",
            delete(handlers::session::delete::<R>),
        )
        // Timeouts
        .route(
            "/session/{session_id}/timeouts",
            get(handlers::timeouts::get::<R>).post(handlers::timeouts::set::<R>),
        )
        // Navigation
        .route(
            "/session/{session_id}/url",
            get(handlers::navigation::get_url::<R>).post(handlers::navigation::navigate::<R>),
        )
        .route(
            "/session/{session_id}/title",
            get(handlers::navigation::get_title::<R>),
        )
        .route(
            "/session/{session_id}/back",
            post(handlers::navigation::back::<R>),
        )
        .route(
            "/session/{session_id}/forward",
            post(handlers::navigation::forward::<R>),
        )
        .route(
            "/session/{session_id}/refresh",
            post(handlers::navigation::refresh::<R>),
        )
        // Elements
        .route(
            "/session/{session_id}/element",
            post(handlers::element::find::<R>),
        )
        .route(
            "/session/{session_id}/elements",
            post(handlers::element::find_all::<R>),
        )
        .route(
            "/session/{session_id}/element/active",
            get(handlers::element::get_active::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/element",
            post(handlers::element::find_from_element::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/elements",
            post(handlers::element::find_all_from_element::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/click",
            post(handlers::element::click::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/clear",
            post(handlers::element::clear::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/value",
            post(handlers::element::send_keys::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/text",
            get(handlers::element::get_text::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/name",
            get(handlers::element::get_tag_name::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/attribute/{name}",
            get(handlers::element::get_attribute::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/property/{name}",
            get(handlers::element::get_property::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/css/{property_name}",
            get(handlers::element::get_css_value::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/rect",
            get(handlers::element::get_rect::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/selected",
            get(handlers::element::is_selected::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/displayed",
            get(handlers::element::is_displayed::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/enabled",
            get(handlers::element::is_enabled::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/computedrole",
            get(handlers::element::get_computed_role::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/computedlabel",
            get(handlers::element::get_computed_label::<R>),
        )
        .route(
            "/session/{session_id}/element/{element_id}/screenshot",
            get(handlers::element::take_screenshot::<R>),
        )
        // Shadow DOM
        .route(
            "/session/{session_id}/element/{element_id}/shadow",
            get(handlers::shadow::get_shadow_root::<R>),
        )
        .route(
            "/session/{session_id}/shadow/{shadow_id}/element",
            post(handlers::shadow::find_element_in_shadow::<R>),
        )
        .route(
            "/session/{session_id}/shadow/{shadow_id}/elements",
            post(handlers::shadow::find_elements_in_shadow::<R>),
        )
        // Execute Script
        .route(
            "/session/{session_id}/execute/sync",
            post(handlers::script::execute_sync::<R>),
        )
        .route(
            "/session/{session_id}/execute/async",
            post(handlers::script::execute_async::<R>),
        )
        // Screenshot
        .route(
            "/session/{session_id}/screenshot",
            get(handlers::screenshot::take::<R>),
        )
        // Document
        .route(
            "/session/{session_id}/source",
            get(handlers::document::get_source::<R>),
        )
        // Window
        .route(
            "/session/{session_id}/window",
            get(handlers::window::get_window_handle::<R>)
                .post(handlers::window::switch_to_window::<R>)
                .delete(handlers::window::close_window::<R>),
        )
        .route(
            "/session/{session_id}/window/new",
            post(handlers::window::new_window::<R>),
        )
        .route(
            "/session/{session_id}/window/handles",
            get(handlers::window::get_window_handles::<R>),
        )
        .route(
            "/session/{session_id}/window/rect",
            get(handlers::window::get_rect::<R>).post(handlers::window::set_rect::<R>),
        )
        .route(
            "/session/{session_id}/window/maximize",
            post(handlers::window::maximize::<R>),
        )
        .route(
            "/session/{session_id}/window/minimize",
            post(handlers::window::minimize::<R>),
        )
        .route(
            "/session/{session_id}/window/fullscreen",
            post(handlers::window::fullscreen::<R>),
        )
        // Frames
        .route(
            "/session/{session_id}/frame",
            post(handlers::frame::switch_to_frame::<R>),
        )
        .route(
            "/session/{session_id}/frame/parent",
            post(handlers::frame::switch_to_parent_frame::<R>),
        )
        // Actions
        .route(
            "/session/{session_id}/actions",
            post(handlers::actions::perform::<R>).delete(handlers::actions::release::<R>),
        )
        // Cookies
        .route(
            "/session/{session_id}/cookie",
            get(handlers::cookie::get_all::<R>)
                .post(handlers::cookie::add::<R>)
                .delete(handlers::cookie::delete_all::<R>),
        )
        .route(
            "/session/{session_id}/cookie/{name}",
            get(handlers::cookie::get::<R>).delete(handlers::cookie::delete::<R>),
        )
        // Alerts
        .route(
            "/session/{session_id}/alert/dismiss",
            post(handlers::alert::dismiss::<R>),
        )
        .route(
            "/session/{session_id}/alert/accept",
            post(handlers::alert::accept::<R>),
        )
        .route(
            "/session/{session_id}/alert/text",
            get(handlers::alert::get_text::<R>).post(handlers::alert::send_text::<R>),
        )
        // Print
        .route(
            "/session/{session_id}/print",
            post(handlers::print::print::<R>),
        )
        .with_state(state)
}
