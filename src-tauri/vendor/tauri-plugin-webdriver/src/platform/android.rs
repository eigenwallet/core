use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Manager, Runtime, WebviewWindow};

use crate::mobile::{
    AlertResult, EvaluateJsArgs, JsResult, ScreenshotArgs, SendAlertTextArgs, TouchArgs, Webdriver,
};
use crate::platform::{
    wrap_script_for_frame_context, Cookie, FrameId, PlatformExecutor, PointerEventType,
    PrintOptions, WindowRect,
};
use crate::server::response::WebDriverErrorResponse;
use crate::webdriver::Timeouts;

/// Android `WebView` executor using Tauri's mobile plugin bridge
#[derive(Clone)]
pub struct AndroidExecutor<R: Runtime> {
    window: WebviewWindow<R>,
    timeouts: Timeouts,
    frame_context: Vec<FrameId>,
}

impl<R: Runtime> AndroidExecutor<R> {
    pub fn new(window: WebviewWindow<R>, timeouts: Timeouts, frame_context: Vec<FrameId>) -> Self {
        Self {
            window,
            timeouts,
            frame_context,
        }
    }
}

// =============================================================================
// Android-specific Plugin Method Arguments
// =============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AsyncScriptArgs {
    async_id: String,
    script: String,
    timeout_ms: u64,
}

#[derive(Debug, Serialize)]
struct GetCookiesArgs {
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetCookieArgs {
    url: String,
    name: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    secure: bool,
    http_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    same_site: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeleteCookieArgs {
    url: String,
    name: String,
}

// =============================================================================
// Android-specific Plugin Method Responses
// =============================================================================

#[derive(Debug, Deserialize)]
struct CookiesResult {
    success: bool,
    cookies: Option<String>, // JSON array as string
    error: Option<String>,
}

#[async_trait]
impl<R: Runtime + 'static> PlatformExecutor<R> for AndroidExecutor<R> {
    fn window(&self) -> &WebviewWindow<R> {
        &self.window
    }

    async fn evaluate_js(&self, script: &str) -> Result<Value, WebDriverErrorResponse> {
        let wrapped_script = wrap_script_for_frame_context(script, &self.frame_context);

        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let args = EvaluateJsArgs {
            script: wrapped_script,
            timeout_ms: self.timeouts.script_ms,
        };

        let result: JsResult = webdriver
            .0
            .run_mobile_plugin_async("evaluateJs", args)
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        if result.success {
            // Parse the stringified JSON value from Android
            let value = if let Some(value_str) = result.value {
                if let Some(s) = value_str.as_str() {
                    // Android returns JSON as a string, parse it
                    serde_json::from_str(s).unwrap_or(value_str)
                } else {
                    value_str
                }
            } else {
                Value::Null
            };

            Ok(serde_json::json!({
                "success": true,
                "value": value
            }))
        } else {
            let error_msg = result.error.as_deref().unwrap_or("Unknown error");
            if error_msg.to_lowercase().contains("timeout") {
                Err(WebDriverErrorResponse::script_timeout())
            } else {
                Err(WebDriverErrorResponse::javascript_error(error_msg, None))
            }
        }
    }

    async fn execute_async_script(
        &self,
        script: &str,
        args: &[Value],
    ) -> Result<Value, WebDriverErrorResponse> {
        let args_json = serde_json::to_string(args)
            .map_err(|e| WebDriverErrorResponse::invalid_argument(&e.to_string()))?;

        let async_id = uuid::Uuid::new_v4().to_string();

        // Build wrapper that includes argument deserialization and callback
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
                var __args = {args_json}.map(deserializeArg);
                __args.push(__done);
                try {{
                    (function() {{ {script} }}).apply(null, __args);
                }} catch (e) {{
                    __done(null, e.message || String(e));
                }}
            }})()"
        );

        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let plugin_args = AsyncScriptArgs {
            async_id,
            script: wrapper,
            timeout_ms: self.timeouts.script_ms,
        };

        let result: JsResult = webdriver
            .0
            .run_mobile_plugin_async("executeAsyncScript", plugin_args)
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        if result.success {
            let value = if let Some(value_str) = result.value {
                if let Some(s) = value_str.as_str() {
                    serde_json::from_str(s).unwrap_or(value_str)
                } else {
                    value_str
                }
            } else {
                Value::Null
            };
            Ok(value)
        } else {
            let error_msg = result.error.as_deref().unwrap_or("Unknown error");
            if error_msg.to_lowercase().contains("timeout") {
                Err(WebDriverErrorResponse::script_timeout())
            } else {
                Err(WebDriverErrorResponse::javascript_error(error_msg, None))
            }
        }
    }

    async fn take_screenshot(&self) -> Result<String, WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let args = ScreenshotArgs {
            timeout_ms: self.timeouts.script_ms,
        };

        let result: JsResult = webdriver
            .0
            .run_mobile_plugin_async("takeScreenshot", args)
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        if result.success {
            if let Some(Value::String(base64)) = result.value {
                Ok(base64)
            } else {
                Err(WebDriverErrorResponse::unknown_error(
                    "Screenshot returned invalid data",
                ))
            }
        } else {
            Err(WebDriverErrorResponse::unknown_error(
                result.error.as_deref().unwrap_or("Screenshot failed"),
            ))
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

        // Take full screenshot (element clipping can be added later)
        self.take_screenshot().await
    }

    async fn print_page(&self, options: PrintOptions) -> Result<String, WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let result: JsResult = webdriver
            .0
            .run_mobile_plugin_async("printToPdf", options)
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        if result.success {
            if let Some(Value::String(base64)) = result.value {
                Ok(base64)
            } else {
                Err(WebDriverErrorResponse::unknown_error(
                    "Print returned invalid data",
                ))
            }
        } else {
            Err(WebDriverErrorResponse::unknown_error(
                result.error.as_deref().unwrap_or("Print failed"),
            ))
        }
    }

    // Override pointer dispatch to use native touch on Android
    async fn dispatch_pointer_event(
        &self,
        event_type: PointerEventType,
        x: i32,
        y: i32,
        _button: u32,
    ) -> Result<(), WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let touch_type = match event_type {
            PointerEventType::Down => "down",
            PointerEventType::Up => "up",
            PointerEventType::Move => "move",
        };

        let args = TouchArgs {
            r#type: touch_type.to_string(),
            x,
            y,
        };

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async("dispatchTouch", args)
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        Ok(())
    }

    // Alert handling via plugin
    async fn get_alert_text(&self) -> Result<String, WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let result: AlertResult = webdriver
            .0
            .run_mobile_plugin_async("getAlertText", ())
            .await
            .map_err(|e| {
                if e.to_string().contains("no such alert") {
                    WebDriverErrorResponse::no_such_alert()
                } else {
                    WebDriverErrorResponse::unknown_error(&e.to_string())
                }
            })?;

        result
            .message
            .ok_or_else(WebDriverErrorResponse::no_such_alert)
    }

    async fn accept_alert(&self) -> Result<(), WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async("acceptAlert", ())
            .await
            .map_err(|e| {
                if e.to_string().contains("no such alert") {
                    WebDriverErrorResponse::no_such_alert()
                } else {
                    WebDriverErrorResponse::unknown_error(&e.to_string())
                }
            })?;

        Ok(())
    }

    async fn dismiss_alert(&self) -> Result<(), WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async("dismissAlert", ())
            .await
            .map_err(|e| {
                if e.to_string().contains("no such alert") {
                    WebDriverErrorResponse::no_such_alert()
                } else {
                    WebDriverErrorResponse::unknown_error(&e.to_string())
                }
            })?;

        Ok(())
    }

    async fn send_alert_text(&self, text: &str) -> Result<(), WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async(
                "sendAlertText",
                SendAlertTextArgs {
                    prompt_text: text.to_string(),
                },
            )
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("no such alert") {
                    WebDriverErrorResponse::no_such_alert()
                } else if msg.contains("not a prompt") {
                    WebDriverErrorResponse::element_not_interactable(
                        "User prompt is not a prompt dialog",
                    )
                } else {
                    WebDriverErrorResponse::unknown_error(&msg)
                }
            })?;

        Ok(())
    }

    // =========================================================================
    // Cookies (using Android CookieManager via plugin)
    // =========================================================================

    async fn get_all_cookies(&self) -> Result<Vec<Cookie>, WebDriverErrorResponse> {
        let url = self
            .window
            .url()
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?
            .to_string();

        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let result: CookiesResult = webdriver
            .0
            .run_mobile_plugin_async("getCookies", GetCookiesArgs { url })
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        if !result.success {
            return Err(WebDriverErrorResponse::unknown_error(
                result.error.as_deref().unwrap_or("Failed to get cookies"),
            ));
        }

        // Parse JSON array of cookies from the plugin
        let cookies_json = result.cookies.unwrap_or_default();
        if cookies_json.is_empty() || cookies_json == "[]" {
            return Ok(Vec::new());
        }

        let cookies: Vec<Cookie> = serde_json::from_str(&cookies_json).map_err(|e| {
            WebDriverErrorResponse::unknown_error(&format!("Failed to parse cookies: {e}"))
        })?;

        Ok(cookies)
    }

    async fn get_cookie(&self, name: &str) -> Result<Option<Cookie>, WebDriverErrorResponse> {
        let cookies = self.get_all_cookies().await?;
        Ok(cookies.into_iter().find(|c| c.name == name))
    }

    async fn add_cookie(&self, mut cookie: Cookie) -> Result<(), WebDriverErrorResponse> {
        let url = self
            .window
            .url()
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        // Per WebDriver spec: if no domain is specified, use the current page's domain
        if cookie.domain.is_none() {
            cookie.domain = url.host_str().map(String::from);
        }

        // Default path to "/" if not specified
        if cookie.path.is_none() {
            cookie.path = Some("/".to_string());
        }

        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async(
                "setCookie",
                SetCookieArgs {
                    url: url.to_string(),
                    name: cookie.name,
                    value: cookie.value,
                    path: cookie.path,
                    domain: cookie.domain,
                    secure: cookie.secure,
                    http_only: cookie.http_only,
                    expiry: cookie.expiry,
                    same_site: cookie.same_site,
                },
            )
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        Ok(())
    }

    async fn delete_cookie(&self, name: &str) -> Result<(), WebDriverErrorResponse> {
        let url = self
            .window
            .url()
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?
            .to_string();

        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async(
                "deleteCookie",
                DeleteCookieArgs {
                    url,
                    name: name.to_string(),
                },
            )
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        Ok(())
    }

    async fn delete_all_cookies(&self) -> Result<(), WebDriverErrorResponse> {
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        let _result: Value = webdriver
            .0
            .run_mobile_plugin_async("deleteAllCookies", ())
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))?;

        Ok(())
    }

    // =========================================================================
    // Window Management
    // =========================================================================

    async fn get_window_rect(&self) -> Result<WindowRect, WebDriverErrorResponse> {
        // Get viewport size from Kotlin plugin
        let webdriver = self.window.app_handle().state::<Webdriver<R>>();

        webdriver
            .0
            .run_mobile_plugin_async("getViewportSize", ())
            .await
            .map_err(|e| WebDriverErrorResponse::unknown_error(&e.to_string()))
    }
}
