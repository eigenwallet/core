use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_webdriver);

// initializes the Kotlin or Swift plugin classes
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Webdriver<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("com.plugin.webdriver", "WebDriverPlugin")?;
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_webdriver)?;
    Ok(Webdriver(handle))
}

/// Access to the webdriver APIs.
pub struct Webdriver<R: Runtime>(pub PluginHandle<R>);

// =============================================================================
// Shared Plugin Method Arguments (Android & iOS)
// =============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateJsArgs {
    pub script: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TouchArgs {
    pub r#type: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotArgs {
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAlertTextArgs {
    pub prompt_text: String,
}

// =============================================================================
// Shared Plugin Method Responses (Android & iOS)
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct JsResult {
    pub success: bool,
    pub value: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AlertResult {
    pub message: Option<String>,
}
