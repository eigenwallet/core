use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Webdriver<R> {
    Webdriver(app.clone())
}

/// Access to the webdriver APIs.
pub struct Webdriver<R: Runtime>(AppHandle<R>);
