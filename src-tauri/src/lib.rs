use std::result::Result;
use std::sync::Arc;
use swap::cli::api::{tauri_bindings::TauriHandle, Context};
use tauri::{Manager, RunEvent};
use tokio::sync::Mutex;

mod commands;

use commands::*;

/// Represents the shared Tauri state. It is accessed by Tauri commands
struct State {
    pub context: Arc<Context>,
    /// Whenever someone wants to modify the context, they should acquire this lock
    ///
    /// [`Context`] uses RwLock internally which means we do not need write access to the context
    /// to modify its internal state.
    ///
    /// However, we want to avoid multiple processes intializing the context at the same time.
    pub context_lock: Mutex<()>,
    pub handle: TauriHandle,
}

impl State {
    /// Creates a new State instance with no Context
    fn new(handle: TauriHandle) -> Self {
        let context = Arc::new(Context::new_with_tauri_handle(handle.clone()));
        let context_lock = Mutex::new(());

        Self {
            context,
            context_lock,
            handle,
        }
    }

    /// Attempts to retrieve the context
    /// Returns an error if the context is not available
    fn context(&self) -> Arc<Context> {
        self.context.clone()
    }
}

/// Sets up the Tauri application
/// Initializes the Tauri state
/// Sets the window title
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Set the window title to include the product name and version
    #[cfg(desktop)]
    {
        let config = app.config();
        let title = format!(
            "{} (v{})",
            config
                .product_name
                .as_ref()
                .expect("Product name to be set"),
            config.version.as_ref().expect("Version to be set")
        );

        let _ = app
            .get_webview_window("main")
            .expect("main window to exist")
            .set_title(&title);
    }

    let app_handle = app.app_handle().to_owned();

    // We need to set a value for the Tauri state right at the start
    // If we don't do this, Tauri commands will panic at runtime if no value is present
    let handle = TauriHandle::new(app_handle.clone());
    let state = State::new(handle);
    app_handle.manage::<State>(state);

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = app
                .get_webview_window("main")
                .expect("no main window")
                .set_focus();
        }));

        builder = builder.plugin(tauri_plugin_cli::init());
    }

    builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_balance,
            get_monero_addresses,
            get_swap_info,
            get_swap_infos_all,
            withdraw_btc,
            buy_xmr,
            resume_swap,
            get_history,
            monero_recovery,
            get_logs,
            list_sellers,
            suspend_current_swap,
            cancel_and_refund,
            initialize_context,
            check_monero_node,
            check_electrum_node,
            get_wallet_descriptor,
            get_current_swap,
            get_data_dir,
            resolve_approval_request,
            redact,
            save_txt_files,
            get_monero_history,
            get_monero_main_address,
            get_monero_balance,
            send_monero,
            get_monero_sync_progress,
            get_monero_seed,
            check_seed,
            get_pending_approvals,
            set_monero_restore_height,
            reject_approval_request,
            get_restore_height,
            dfx_authenticate,
            change_monero_node,
            get_context_status
        ])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::Exit | RunEvent::ExitRequested { .. } => {
                // Here we cleanup the Context when the application is closed
                // This is necessary to among other things stop the monero-wallet-rpc process
                // If the application is forcibly closed, this may not be called.
                // TODO: fix that
                let state = app.state::<State>();
                let lock = state.context_lock.try_lock();
                if let Ok(_) = lock {
                    if let Err(e) = state.context().cleanup() {
                        println!("Failed to cleanup context: {}", e);
                    }
                }
            }
            _ => {}
        })
}
