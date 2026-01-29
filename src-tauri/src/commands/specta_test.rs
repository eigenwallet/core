use specta::specta;

#[tauri::command]
#[specta]
pub fn hello_specta(name: String) -> String {
    format!("Hello, {}! This is a command generated via Tauri-Specta.", name)
}
