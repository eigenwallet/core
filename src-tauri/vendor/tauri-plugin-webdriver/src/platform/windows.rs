use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde_json::Value;
use tauri::{Manager, Runtime, WebviewWindow};
use tokio::sync::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2CapturePreviewCompletedHandler, ICoreWebView2Environment6,
    ICoreWebView2ExecuteScriptCompletedHandler, ICoreWebView2PrintToPdfCompletedHandler,
    ICoreWebView2ScriptDialogOpeningEventHandler, ICoreWebView2WebMessageReceivedEventHandler,
    ICoreWebView2_7, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE, COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
};
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{
    CoInitializeEx, COINIT_APARTMENTTHREADED, STATFLAG_NONAME, STREAM_SEEK_SET,
};
use windows_core::BOOL;

use crate::platform::alert_state::{AlertState, AlertStateManager, AlertType, PendingAlert};
use crate::platform::{wrap_script_for_frame_context, FrameId, PlatformExecutor, PrintOptions};
use crate::server::response::WebDriverErrorResponse;
use crate::webdriver::Timeouts;

// =============================================================================
// Async Script State
// =============================================================================

/// Handler name used for postMessage calls
const HANDLER_NAME: &str = "webdriver_async";

/// Shared state for pending async script operations.
/// This is managed via Tauri's state system (`app.manage()`).
#[derive(Default)]
pub struct AsyncScriptState {
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    /// Track which webviews have native handlers registered (by window label)
    registered_handlers: Mutex<HashSet<String>>,
}

impl AsyncScriptState {
    /// Register a pending async operation and return the receiver
    pub fn register(&self, id: String) -> oneshot::Receiver<Result<Value, String>> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, tx);
        }
        rx
    }

    /// Complete a pending async operation with a result
    pub fn complete(&self, id: &str, result: Result<Value, String>) {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(tx) = pending.remove(id) {
                let _ = tx.send(result);
            }
        }
    }

    /// Cancel a pending async operation
    pub fn cancel(&self, id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }

    /// Check if a handler is registered for a window label, and mark it as registered if not.
    /// Returns true if the handler was already registered, false if it needs to be registered.
    pub fn mark_handler_registered(&self, label: &str) -> bool {
        if let Ok(mut handlers) = self.registered_handlers.lock() {
            !handlers.insert(label.to_string())
        } else {
            false
        }
    }
}

/// Wrapper for raw COM pointer to allow sending across threads.
/// SAFETY: The COM object must only be accessed from a COM-initialized thread.
struct SendableComPtr(*mut std::ffi::c_void);
unsafe impl Send for SendableComPtr {}

impl SendableComPtr {
    fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.0
    }
}

/// Windows `WebView2` executor
#[derive(Clone)]
pub struct WindowsExecutor<R: Runtime> {
    window: WebviewWindow<R>,
    timeouts: Timeouts,
    frame_context: Vec<FrameId>,
}

impl<R: Runtime> WindowsExecutor<R> {
    pub fn new(window: WebviewWindow<R>, timeouts: Timeouts, frame_context: Vec<FrameId>) -> Self {
        Self {
            window,
            timeouts,
            frame_context,
        }
    }
}

/// Register `WebView2` handlers at webview creation time.
/// This is called from the plugin's `on_webview_ready` hook to ensure
/// the script dialog handler is registered before any navigation completes.
pub fn register_webview_handlers<R: Runtime>(webview: &tauri::Webview<R>) {
    // Get per-window alert state from the manager
    let manager = webview.app_handle().state::<AlertStateManager>();
    let alert_state = manager.get_or_create(webview.label());

    let _ = webview.with_webview(move |webview| unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        if let Ok(webview2) = webview.controller().CoreWebView2() {
            // Disable default script dialogs so ScriptDialogOpening event fires
            if let Ok(settings) = webview2.Settings() {
                if let Err(e) = settings.SetAreDefaultScriptDialogsEnabled(false) {
                    tracing::error!("Failed to disable default script dialogs: {e:?}");
                    return;
                }
            } else {
                tracing::error!("Failed to get webview settings");
                return;
            }

            let handler: ICoreWebView2ScriptDialogOpeningEventHandler =
                ScriptDialogOpeningHandler::new(alert_state).into();

            let mut token = std::mem::zeroed();
            if let Err(e) = webview2.add_ScriptDialogOpening(&handler, &raw mut token) {
                tracing::error!("Failed to register ScriptDialogOpening handler: {e:?}");
            } else {
                tracing::debug!("Registered script dialog handler for webview");
            }

            // Prevent handler from being dropped - leak it to keep the COM ref alive
            std::mem::forget(handler);
        }
    });
}

#[async_trait]
impl<R: Runtime + 'static> PlatformExecutor<R> for WindowsExecutor<R> {
    // =========================================================================
    // Window Access
    // =========================================================================

    fn window(&self) -> &WebviewWindow<R> {
        &self.window
    }

    // =========================================================================
    // Core JavaScript Execution
    // =========================================================================

    async fn evaluate_js(&self, script: &str) -> Result<Value, WebDriverErrorResponse> {
        let (tx, rx) = oneshot::channel();
        let script_owned = wrap_script_for_frame_context(script, &self.frame_context);

        let result = self.window.with_webview(move |webview| unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            if let Ok(webview2) = webview.controller().CoreWebView2() {
                let script_hstring = HSTRING::from(&script_owned);

                let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
                let handler: ICoreWebView2ExecuteScriptCompletedHandler =
                    ExecuteScriptHandler::new(tx).into();

                webview2
                    .ExecuteScript(PCWSTR(script_hstring.as_ptr()), &handler)
                    .ok();
            }
        });

        if let Err(e) = result {
            return Err(WebDriverErrorResponse::javascript_error(
                &e.to_string(),
                None,
            ));
        }

        let timeout = std::time::Duration::from_millis(self.timeouts.script_ms);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(serde_json::json!({
                "success": true,
                "value": value
            })),
            Ok(Ok(Err(error))) => Err(WebDriverErrorResponse::javascript_error(&error, None)),
            Ok(Err(_)) => Err(WebDriverErrorResponse::unknown_error("Channel closed")),
            Err(_) => Err(WebDriverErrorResponse::script_timeout()),
        }
    }

    // =========================================================================
    // Screenshots
    // =========================================================================

    async fn take_screenshot(&self) -> Result<String, WebDriverErrorResponse> {
        // Use WebView2's native CapturePreview API
        let (tx, rx) = oneshot::channel();

        let result = self.window.with_webview(move |webview| {
            unsafe {
                if let Ok(webview2) = webview.controller().CoreWebView2() {
                    // Create an in-memory stream for the PNG image
                    let stream = match CreateStreamOnHGlobal(HGLOBAL::default(), true) {
                        Ok(s) => s,
                        Err(e) => {
                            let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
                            if let Ok(mut guard) = tx.lock() {
                                if let Some(tx) = guard.take() {
                                    let _ = tx.send(Err(format!("Failed to create stream: {e}")));
                                }
                            }
                            return;
                        }
                    };

                    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
                    let handler = CapturePreviewHandler::new(tx, stream.clone());
                    let handler: ICoreWebView2CapturePreviewCompletedHandler = handler.into();

                    // Capture the preview as PNG
                    if let Err(e) = webview2.CapturePreview(
                        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                        &stream,
                        &handler,
                    ) {
                        // Handler won't be called, manually signal error
                        // Note: handler already moved, so we can't access tx directly
                        tracing::error!("CapturePreview failed: {e}");
                    }
                }
            }
        });

        if let Err(e) = result {
            return Err(WebDriverErrorResponse::unknown_error(&e.to_string()));
        }

        let timeout = std::time::Duration::from_millis(self.timeouts.script_ms);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(base64))) => {
                if base64.is_empty() {
                    Err(WebDriverErrorResponse::unknown_error(
                        "Screenshot returned empty data",
                    ))
                } else {
                    Ok(base64)
                }
            }
            Ok(Ok(Err(error))) => Err(WebDriverErrorResponse::unknown_error(&error)),
            Ok(Err(_)) => Err(WebDriverErrorResponse::unknown_error("Channel closed")),
            Err(_) => Err(WebDriverErrorResponse::script_timeout()),
        }
    }

    async fn take_element_screenshot(
        &self,
        js_var: &str,
    ) -> Result<String, WebDriverErrorResponse> {
        // Scroll element into view first
        let script = format!(
            r"(function() {{
                var el = window.{js_var};
                if (!el || !el.isConnected) {{
                    throw new Error('stale element reference');
                }}
                el.scrollIntoView({{ block: 'center', inline: 'center' }});
                return true;
            }})()"
        );
        self.evaluate_js(&script).await?;

        // Take full screenshot and return (element clipping can be added later)
        self.take_screenshot().await
    }

    // =========================================================================
    // Print
    // =========================================================================

    #[allow(clippy::too_many_lines)]
    async fn print_page(&self, options: PrintOptions) -> Result<String, WebDriverErrorResponse> {
        let (tx, rx) = oneshot::channel();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

        // Create temp directory for PDF output (auto-cleanup on drop)
        // Note: We use TempDir instead of NamedTempFile because NamedTempFile
        // opens/locks the file on Windows, preventing WebView2 from writing to it
        let temp_dir = tempfile::TempDir::new().map_err(|e| {
            WebDriverErrorResponse::unknown_error(&format!("Failed to create temp dir: {e}"))
        })?;
        let pdf_path = temp_dir.path().join("print.pdf");
        let pdf_path_clone = pdf_path.clone();

        // Extract options before moving into closure
        let orientation = options.orientation.clone();
        let scale = options.scale;
        let background = options.background;
        let page_width = options.page_width;
        let page_height = options.page_height;
        let margin_top = options.margin_top;
        let margin_bottom = options.margin_bottom;
        let margin_left = options.margin_left;
        let margin_right = options.margin_right;

        let result = self.window.with_webview(move |webview| unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let webview2 = match webview.controller().CoreWebView2() {
                Ok(wv) => wv,
                Err(e) => {
                    if let Ok(mut guard) = tx.lock() {
                        if let Some(tx) = guard.take() {
                            let _ = tx.send(Err(format!("Failed to get CoreWebView2: {e:?}")));
                        }
                    }
                    return;
                }
            };

            // Cast to ICoreWebView2_7 for PrintToPdf support
            let webview7: ICoreWebView2_7 = match webview2.cast() {
                Ok(wv) => wv,
                Err(e) => {
                    if let Ok(mut guard) = tx.lock() {
                        if let Some(tx) = guard.take() {
                            let _ =
                                tx.send(Err(format!("Failed to cast to ICoreWebView2_7: {e:?}")));
                        }
                    }
                    return;
                }
            };

            // Get environment to create print settings
            let environment = match webview7.Environment() {
                Ok(env) => env,
                Err(e) => {
                    if let Ok(mut guard) = tx.lock() {
                        if let Some(tx) = guard.take() {
                            let _ = tx.send(Err(format!("Failed to get environment: {e:?}")));
                        }
                    }
                    return;
                }
            };

            // Cast to ICoreWebView2Environment6 for CreatePrintSettings
            let env6: ICoreWebView2Environment6 = match environment.cast() {
                Ok(env) => env,
                Err(e) => {
                    if let Ok(mut guard) = tx.lock() {
                        if let Some(tx) = guard.take() {
                            let _ = tx.send(Err(format!(
                                "Failed to cast to ICoreWebView2Environment6: {e:?}"
                            )));
                        }
                    }
                    return;
                }
            };

            // Create print settings
            let settings = match env6.CreatePrintSettings() {
                Ok(s) => s,
                Err(e) => {
                    if let Ok(mut guard) = tx.lock() {
                        if let Some(tx) = guard.take() {
                            let _ = tx.send(Err(format!("Failed to create print settings: {e:?}")));
                        }
                    }
                    return;
                }
            };

            // Apply print options
            // Orientation
            if let Some(ref orient) = orientation {
                let orientation_val = if orient == "landscape" {
                    COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE
                } else {
                    COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT
                };
                let _ = settings.SetOrientation(orientation_val);
            }

            // Scale factor (1.0 = 100%)
            if let Some(s) = scale {
                let _ = settings.SetScaleFactor(s);
            }

            // Print backgrounds
            if let Some(bg) = background {
                let _ = settings.SetShouldPrintBackgrounds(bg);
            }

            // Page dimensions (WebDriver uses cm, WebView2 uses inches)
            // 1 inch = 2.54 cm
            if let Some(w) = page_width {
                let _ = settings.SetPageWidth(w / 2.54);
            }
            if let Some(h) = page_height {
                let _ = settings.SetPageHeight(h / 2.54);
            }

            // Margins (WebDriver uses cm, WebView2 uses inches)
            if let Some(m) = margin_top {
                let _ = settings.SetMarginTop(m / 2.54);
            }
            if let Some(m) = margin_bottom {
                let _ = settings.SetMarginBottom(m / 2.54);
            }
            if let Some(m) = margin_left {
                let _ = settings.SetMarginLeft(m / 2.54);
            }
            if let Some(m) = margin_right {
                let _ = settings.SetMarginRight(m / 2.54);
            }

            // Create completion handler
            let handler: ICoreWebView2PrintToPdfCompletedHandler =
                handlers::PrintToPdfHandler::new(tx).into();

            // Convert path to HSTRING
            let path_str = pdf_path_clone.to_string_lossy().to_string();
            let path_hstring = HSTRING::from(&path_str);

            // Call PrintToPdf
            if let Err(e) = webview7.PrintToPdf(&path_hstring, &settings, &handler) {
                tracing::error!("PrintToPdf call failed: {e:?}");
            }
        });

        if let Err(e) = result {
            return Err(WebDriverErrorResponse::unknown_error(&e.to_string()));
        }

        // Wait for completion
        let timeout = std::time::Duration::from_millis(self.timeouts.script_ms);
        let print_result = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(WebDriverErrorResponse::unknown_error(&error)),
            Ok(Err(_)) => Err(WebDriverErrorResponse::unknown_error("Channel closed")),
            Err(_) => Err(WebDriverErrorResponse::script_timeout()),
        };

        // Check if print succeeded
        print_result?;

        // Read the PDF file (temp_file auto-cleans on drop)
        let pdf_data = std::fs::read(&pdf_path).map_err(|e| {
            WebDriverErrorResponse::unknown_error(&format!("Failed to read PDF file: {e}"))
        })?;

        Ok(BASE64_STANDARD.encode(&pdf_data))
    }

    // =========================================================================
    // Async Script Execution
    // =========================================================================

    async fn execute_async_script(
        &self,
        script: &str,
        args: &[Value],
    ) -> Result<Value, WebDriverErrorResponse> {
        let args_json = serde_json::to_string(args)
            .map_err(|e| WebDriverErrorResponse::invalid_argument(&e.to_string()))?;

        let async_id = uuid::Uuid::new_v4().to_string();

        // Get async state and register this operation
        let app = self.window.app_handle().clone();
        let async_state = app.state::<AsyncScriptState>();
        let label = self.window.label().to_string();

        // Register handler if not already registered for this window
        if !async_state.mark_handler_registered(&label) {
            let app_clone = app.clone();
            let handler_result = self.window.with_webview(move |webview| unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                if let Ok(webview2) = webview.controller().CoreWebView2() {
                    let state = app_clone.state::<AsyncScriptState>();
                    register_message_handler(&webview2, state.inner());
                }
            });

            if let Err(e) = handler_result {
                return Err(WebDriverErrorResponse::unknown_error(&format!(
                    "Failed to register message handler: {e}"
                )));
            }
        }

        let rx = async_state.register(async_id.clone());

        // Build wrapper script using postMessage
        let wrapper = format!(
            r"(function() {{
                var ELEMENT_KEY = 'element-6066-11e4-a52e-4f735466cecf';
                function deserializeArg(arg) {{
                    if (arg === null || arg === undefined) return arg;
                    if (Array.isArray(arg)) return arg.map(deserializeArg);
                    if (typeof arg === 'object') {{
                        if (arg[ELEMENT_KEY]) {{
                            var el = window['__wd_el_' + arg[ELEMENT_KEY].replace(/-/g, '')];
                            if (!el) throw new Error('stale element reference');
                            return el;
                        }}
                        var result = {{}};
                        for (var key in arg) {{
                            if (arg.hasOwnProperty(key)) result[key] = deserializeArg(arg[key]);
                        }}
                        return result;
                    }}
                    return arg;
                }}
                var __done = function(r) {{
                    window.chrome.webview.postMessage(JSON.stringify({{
                        handler: '{HANDLER_NAME}',
                        id: '{async_id}',
                        result: r,
                        error: null
                    }}));
                }};
                var __args = {args_json}.map(deserializeArg);
                __args.push(__done);
                try {{
                    (function() {{ {script} }}).apply(null, __args);
                }} catch (e) {{
                    window.chrome.webview.postMessage(JSON.stringify({{
                        handler: '{HANDLER_NAME}',
                        id: '{async_id}',
                        result: null,
                        error: e.message || String(e)
                    }}));
                }}
            }})()"
        );

        // Execute the wrapper (returns immediately)
        self.evaluate_js(&wrapper).await?;

        // Wait for result with timeout
        let timeout_ms = self.timeouts.script_ms;
        let timeout = std::time::Duration::from_millis(timeout_ms);

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(WebDriverErrorResponse::javascript_error(&error, None)),
            Ok(Err(_)) => Err(WebDriverErrorResponse::unknown_error("Channel closed")),
            Err(_) => {
                async_state.cancel(&async_id);
                Err(WebDriverErrorResponse::script_timeout())
            }
        }
    }
}

// =============================================================================
// Helper Methods
// =============================================================================
// COM Handlers
// =============================================================================

type ScriptResultSender = Arc<std::sync::Mutex<Option<oneshot::Sender<Result<Value, String>>>>>;
type CaptureResultSender = Arc<std::sync::Mutex<Option<oneshot::Sender<Result<String, String>>>>>;
type PrintResultSender = Arc<std::sync::Mutex<Option<oneshot::Sender<Result<(), String>>>>>;

mod handlers {
    #![allow(clippy::inline_always, clippy::ref_as_ptr)]

    use serde_json::Value;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2CapturePreviewCompletedHandler,
        ICoreWebView2CapturePreviewCompletedHandler_Impl, ICoreWebView2Deferral,
        ICoreWebView2ExecuteScriptCompletedHandler,
        ICoreWebView2ExecuteScriptCompletedHandler_Impl, ICoreWebView2PrintToPdfCompletedHandler,
        ICoreWebView2PrintToPdfCompletedHandler_Impl, ICoreWebView2ScriptDialogOpeningEventArgs,
        ICoreWebView2ScriptDialogOpeningEventHandler,
        ICoreWebView2ScriptDialogOpeningEventHandler_Impl,
        ICoreWebView2WebMessageReceivedEventArgs, ICoreWebView2WebMessageReceivedEventHandler,
        ICoreWebView2WebMessageReceivedEventHandler_Impl, COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT,
        COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM, COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT,
    };
    use windows::core::{implement, Interface};

    use super::{
        AlertState, AlertType, AsyncScriptState, CaptureResultSender, PendingAlert,
        PrintResultSender, ScriptResultSender, SendableComPtr, HANDLER_NAME,
    };
    use crate::platform::alert_state::AlertResponse;
    use std::sync::Arc;

    #[implement(ICoreWebView2ExecuteScriptCompletedHandler)]
    pub struct ExecuteScriptHandler {
        pub tx: ScriptResultSender,
    }

    impl ExecuteScriptHandler {
        pub fn new(tx: ScriptResultSender) -> Self {
            Self { tx }
        }
    }

    impl ICoreWebView2ExecuteScriptCompletedHandler_Impl for ExecuteScriptHandler_Impl {
        fn Invoke(
            &self,
            errorcode: windows::core::HRESULT,
            resultobjectasjson: &windows::core::PCWSTR,
        ) -> windows::core::Result<()> {
            let response = if errorcode.is_err() {
                Err(format!("Script execution failed: {errorcode:?}"))
            } else {
                let json_str = unsafe { resultobjectasjson.to_string().unwrap_or_default() };
                match serde_json::from_str(&json_str) {
                    Ok(value) => Ok(value),
                    Err(_) => Ok(Value::String(json_str)),
                }
            };

            if let Ok(mut guard) = self.tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(response);
                }
            }
            Ok(())
        }
    }

    #[implement(ICoreWebView2CapturePreviewCompletedHandler)]
    pub struct CapturePreviewHandler {
        pub tx: CaptureResultSender,
        pub stream: windows::Win32::System::Com::IStream,
    }

    impl CapturePreviewHandler {
        pub fn new(tx: CaptureResultSender, stream: windows::Win32::System::Com::IStream) -> Self {
            Self { tx, stream }
        }
    }

    impl ICoreWebView2CapturePreviewCompletedHandler_Impl for CapturePreviewHandler_Impl {
        fn Invoke(&self, errorcode: windows::core::HRESULT) -> windows::core::Result<()> {
            let response = if errorcode.is_err() {
                Err(format!("Capture preview failed: {errorcode:?}"))
            } else {
                // Read PNG data from the stream
                unsafe {
                    use super::{STATFLAG_NONAME, STREAM_SEEK_SET};

                    // Get stream size
                    let mut stat = std::mem::zeroed();
                    if self.stream.Stat(&mut stat, STATFLAG_NONAME).is_err() {
                        return Ok(());
                    }
                    let size = stat.cbSize as usize;

                    if size == 0 {
                        if let Ok(mut guard) = self.tx.lock() {
                            if let Some(tx) = guard.take() {
                                let _ = tx.send(Err("Empty stream".to_string()));
                            }
                        }
                        return Ok(());
                    }

                    // Seek to beginning
                    let _ = self.stream.Seek(0, STREAM_SEEK_SET, None);

                    // Read data
                    let mut buffer = vec![0u8; size];
                    let mut bytes_read = 0u32;
                    if self
                        .stream
                        .Read(
                            buffer.as_mut_ptr().cast(),
                            size as u32,
                            Some(&mut bytes_read),
                        )
                        .is_err()
                    {
                        if let Ok(mut guard) = self.tx.lock() {
                            if let Some(tx) = guard.take() {
                                let _ = tx.send(Err("Failed to read stream".to_string()));
                            }
                        }
                        return Ok(());
                    }

                    buffer.truncate(bytes_read as usize);

                    // Encode as base64
                    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
                    use base64::Engine as _;
                    let base64 = BASE64_STANDARD.encode(&buffer);

                    if let Ok(mut guard) = self.tx.lock() {
                        if let Some(tx) = guard.take() {
                            let _ = tx.send(Ok(base64));
                        }
                    }
                    return Ok(());
                }
            };

            if let Ok(mut guard) = self.tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(response);
                }
            }
            Ok(())
        }
    }

    /// Handler for PDF printing completion
    #[implement(ICoreWebView2PrintToPdfCompletedHandler)]
    pub struct PrintToPdfHandler {
        pub tx: PrintResultSender,
    }

    impl PrintToPdfHandler {
        pub fn new(tx: PrintResultSender) -> Self {
            Self { tx }
        }
    }

    impl ICoreWebView2PrintToPdfCompletedHandler_Impl for PrintToPdfHandler_Impl {
        fn Invoke(
            &self,
            errorcode: windows::core::HRESULT,
            issuccessful: super::BOOL,
        ) -> windows::core::Result<()> {
            let response = if errorcode.is_err() {
                Err(format!("PrintToPdf failed: {errorcode:?}"))
            } else if !issuccessful.as_bool() {
                Err("PrintToPdf was not successful".to_string())
            } else {
                Ok(())
            };

            if let Ok(mut guard) = self.tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(response);
                }
            }
            Ok(())
        }
    }

    /// Handler for receiving web messages from JavaScript via postMessage
    #[implement(ICoreWebView2WebMessageReceivedEventHandler)]
    pub struct WebMessageReceivedHandler {
        state_ptr: *const AsyncScriptState,
    }

    // SAFETY: The state pointer is valid for the lifetime of the app (managed by Tauri)
    unsafe impl Send for WebMessageReceivedHandler {}
    unsafe impl Sync for WebMessageReceivedHandler {}

    impl WebMessageReceivedHandler {
        pub fn new(state: &AsyncScriptState) -> Self {
            Self {
                state_ptr: state as *const AsyncScriptState,
            }
        }
    }

    impl ICoreWebView2WebMessageReceivedEventHandler_Impl for WebMessageReceivedHandler_Impl {
        fn Invoke(
            &self,
            _sender: windows::core::Ref<'_, ICoreWebView2>,
            args: windows::core::Ref<'_, ICoreWebView2WebMessageReceivedEventArgs>,
        ) -> windows::core::Result<()> {
            unsafe {
                let state_ptr = self.state_ptr;
                if state_ptr.is_null() {
                    tracing::error!("AsyncScriptState pointer is null");
                    return Ok(());
                }
                let state = &*state_ptr;

                // Get the message as JSON string
                let Some(args_owned) = args.clone() else {
                    return Ok(());
                };
                let mut msg_ptr = windows::core::PWSTR::null();
                if args_owned.WebMessageAsJson(&raw mut msg_ptr).is_err() {
                    return Ok(()); // Failed to get message
                }
                let msg_text = msg_ptr.to_string().unwrap_or_default();

                // `WebMessageAsJson` returns the message as a JSON value.
                // Since JS sends `JSON.stringify({...})`, the message is a string,
                // so we get a JSON-encoded string (with extra quotes).
                // First parse to get the inner string, then parse that as our object.
                let inner_str: String = match serde_json::from_str(&msg_text) {
                    Ok(s) => s,
                    Err(_) => return Ok(()), // Not a JSON string
                };
                let msg: Value = match serde_json::from_str(&inner_str) {
                    Ok(v) => v,
                    Err(_) => return Ok(()), // Not our message format
                };

                // Check if this is our handler
                let handler = msg.get("handler").and_then(Value::as_str);
                if handler != Some(HANDLER_NAME) {
                    return Ok(()); // Not for us
                }

                // Extract async ID
                let Some(async_id) = msg.get("id").and_then(Value::as_str) else {
                    tracing::warn!("Message missing 'id' field");
                    return Ok(());
                };
                let async_id = async_id.to_string();

                // Check for error
                if let Some(error) = msg.get("error").and_then(Value::as_str) {
                    if !error.is_empty() {
                        state.complete(&async_id, Err(error.to_string()));
                        return Ok(());
                    }
                }

                // Extract result
                let result = msg.get("result").cloned().unwrap_or(Value::Null);
                state.complete(&async_id, Ok(result));
            }
            Ok(())
        }
    }

    /// Handler for intercepting JavaScript alert/confirm/prompt dialogs
    #[implement(ICoreWebView2ScriptDialogOpeningEventHandler)]
    pub struct ScriptDialogOpeningHandler {
        alert_state: Arc<AlertState>,
    }

    // SAFETY: Arc<AlertState> is Send + Sync
    unsafe impl Send for ScriptDialogOpeningHandler {}
    unsafe impl Sync for ScriptDialogOpeningHandler {}

    impl ScriptDialogOpeningHandler {
        pub fn new(alert_state: Arc<AlertState>) -> Self {
            Self { alert_state }
        }
    }

    impl ICoreWebView2ScriptDialogOpeningEventHandler_Impl for ScriptDialogOpeningHandler_Impl {
        fn Invoke(
            &self,
            _sender: windows::core::Ref<'_, ICoreWebView2>,
            args: windows::core::Ref<'_, ICoreWebView2ScriptDialogOpeningEventArgs>,
        ) -> windows::core::Result<()> {
            // Extract data and prepare for async handling inside unsafe block
            let (args_ptr, deferral_ptr, rx) = unsafe {
                let Some(args) = args.clone() else {
                    return Ok(());
                };

                // Get dialog kind
                let mut kind = std::mem::zeroed();
                if args.Kind(&raw mut kind).is_err() {
                    tracing::error!("Failed to get script dialog kind");
                    return Ok(());
                }

                // Get message
                let mut message_ptr = windows::core::PWSTR::null();
                if args.Message(&raw mut message_ptr).is_err() {
                    tracing::error!("Failed to get script dialog message");
                    return Ok(());
                }
                let message = message_ptr.to_string().unwrap_or_default();

                // Get default text for prompts
                let mut default_text_ptr = windows::core::PWSTR::null();
                let default_text = if args.DefaultText(&raw mut default_text_ptr).is_ok() {
                    let text = default_text_ptr.to_string().unwrap_or_default();
                    if text.is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                } else {
                    None
                };

                // Map WebView2 dialog kind to our AlertType
                let alert_type = if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT {
                    AlertType::Alert
                } else if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM {
                    AlertType::Confirm
                } else if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT {
                    AlertType::Prompt
                } else {
                    // BEFOREUNLOAD or unknown - just accept it
                    let _ = args.Accept();
                    return Ok(());
                };

                tracing::debug!("Intercepted {:?} dialog: {}", alert_type, message);

                // Get deferral to handle asynchronously (avoid blocking UI thread)
                let deferral = match args.GetDeferral() {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("Failed to get deferral: {e:?}");
                        let _ = args.Accept();
                        return Ok(());
                    }
                };

                // Create channel for WebDriver response
                let (tx, rx) = std::sync::mpsc::channel::<AlertResponse>();
                self.alert_state.set_pending(PendingAlert {
                    message: message.clone(),
                    default_text: default_text.clone(),
                    alert_type,
                    responder: tx,
                });

                // Wrap COM objects for thread transfer
                let args_ptr = SendableComPtr(args.into_raw());
                let deferral_ptr = SendableComPtr(deferral.into_raw());

                (args_ptr, deferral_ptr, rx)
            };

            // Spawn thread to wait for WebDriver response (don't block UI thread)
            std::thread::spawn(move || {
                let timeout = std::time::Duration::from_secs(30);
                let response = rx.recv_timeout(timeout);

                // SAFETY: These pointers came from valid COM objects and we're
                // accessing them from a single thread. All COM method calls are unsafe.
                unsafe {
                    let args =
                        ICoreWebView2ScriptDialogOpeningEventArgs::from_raw(args_ptr.as_ptr());
                    let deferral = ICoreWebView2Deferral::from_raw(deferral_ptr.as_ptr());

                    match response {
                        Ok(AlertResponse {
                            accepted,
                            prompt_text,
                        }) => {
                            if accepted {
                                // Set prompt text if provided
                                if let Some(text) = prompt_text {
                                    let result = windows::core::HSTRING::from(text.as_str());
                                    let _ =
                                        args.SetResultText(windows::core::PCWSTR(result.as_ptr()));
                                }
                                let _ = args.Accept();
                            }
                            // If not accepted, don't call Accept() - dialog returns false/null
                        }
                        Err(_) => {
                            // Timeout - auto-accept
                            let _ = args.Accept();
                        }
                    }

                    // Complete the deferral to let WebView2 continue
                    let _ = deferral.Complete();
                }
            });

            Ok(())
        }
    }
}

use handlers::{
    CapturePreviewHandler, ExecuteScriptHandler, ScriptDialogOpeningHandler,
    WebMessageReceivedHandler,
};

// =============================================================================
// Native Message Handler Registration
// =============================================================================

/// Register the `WebMessage` handler for a webview.
///
/// # Safety
/// Must be called from a COM-initialized thread with a valid webview.
unsafe fn register_message_handler(webview: &ICoreWebView2, state: &AsyncScriptState) {
    let handler: ICoreWebView2WebMessageReceivedEventHandler =
        WebMessageReceivedHandler::new(state).into();

    // We don't need to store the token since we never remove the handler
    let mut token = std::mem::zeroed();
    if let Err(e) = webview.add_WebMessageReceived(&handler, &raw mut token) {
        tracing::error!("Failed to register WebMessageReceived handler: {e:?}");
    } else {
        tracing::debug!("Registered native message handler for webview");
    }
}
