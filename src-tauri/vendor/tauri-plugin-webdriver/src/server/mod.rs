use std::net::SocketAddr;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};
use tokio::runtime::Runtime as TokioRuntime;
use tokio::sync::RwLock;

pub mod handlers;
pub mod response;
pub mod router;

use crate::platform::{create_executor, FrameId, PlatformExecutor};
use crate::server::response::WebDriverErrorResponse;
use crate::webdriver::{SessionManager, Timeouts};

/// Shared state for the `WebDriver` server
pub struct AppState<R: Runtime> {
    pub app: AppHandle<R>,
    pub sessions: RwLock<SessionManager>,
}

impl<R: Runtime + 'static> AppState<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            sessions: RwLock::new(SessionManager::new()),
        }
    }

    /// Get a platform executor for a specific window by label
    pub fn get_executor_for_window(
        &self,
        window_label: &str,
        timeouts: Timeouts,
        frame_context: Vec<FrameId>,
    ) -> Result<Arc<dyn PlatformExecutor<R>>, WebDriverErrorResponse> {
        self.app
            .webview_windows()
            .get(window_label)
            .cloned()
            .map(|window| create_executor(window, timeouts, frame_context))
            .ok_or_else(WebDriverErrorResponse::no_such_window)
    }

    /// Get all window labels
    pub fn get_window_labels(&self) -> Vec<String> {
        self.app.webview_windows().keys().cloned().collect()
    }
}

/// Start the `WebDriver` HTTP server on the specified port
pub fn start<R: Runtime + 'static>(app: AppHandle<R>, port: u16) {
    std::thread::spawn(move || {
        let rt = TokioRuntime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async {
            let state = Arc::new(AppState::new(app));
            let router = router::create_router(state);

            // On Android, bind to all interfaces for WiFi accessibility
            // On other platforms, bind to localhost only for security
            #[cfg(target_os = "android")]
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            #[cfg(not(target_os = "android"))]
            let addr = SocketAddr::from(([127, 0, 0, 1], port));

            tracing::info!("WebDriver server listening on http://{}", addr);

            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("Failed to bind to address");

            axum::serve(listener, router).await.expect("Server error");
        });
    });
}
