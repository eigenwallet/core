use std::result::Result;
use std::sync::Arc;
use swap::cli::{
    api::{
        request::{
            BalanceArgs, BuyXmrArgs, GetHistoryArgs, GetSwapInfosAllArgs, ResumeSwapArgs,
            SuspendCurrentSwapArgs, WithdrawBtcArgs,
        },
        tauri_bindings::TauriHandle,
        Context, ContextBuilder,
    },
    command::{Bitcoin, Monero},
};
use tauri::{Manager, RunEvent};

trait ToStringResult<T> {
    fn to_string_result(self) -> Result<T, String>;
}

// Implement the trait for Result<T, E>
impl<T, E: ToString> ToStringResult<T> for Result<T, E> {
    fn to_string_result(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

/// This macro is used to create boilerplate functions as tauri commands
/// that simply delegate handling to the respective request type.
///
/// # Example
/// ```ignored
/// tauri_command!(get_balance, BalanceArgs);
/// ```
/// will resolve to
/// ```ignored
/// #[tauri::command]
/// async fn get_balance(context: tauri::State<'...>, args: BalanceArgs) -> Result<BalanceArgs::Response, String> {
///     args.handle(context.inner().clone()).await.to_string_result()
/// }
///
/// # Example 2
/// ```ignored
/// tauri_command!(get_balance, BalanceArgs, no_args);
/// ```
/// will resolve to
/// ```ignored
/// #[tauri::command]
/// async fn get_balance(context: tauri::State<'...>) -> Result<BalanceArgs::Response, String> {
///    BalanceArgs {}.handle(context.inner().clone()).await.to_string_result()
/// }
/// ```
macro_rules! tauri_command {
    ($fn_name:ident, $request_name:ident) => {
        #[tauri::command]
        async fn $fn_name(
            context: tauri::State<'_, Option<Arc<Context>>>,
            args: $request_name,
        ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
            // Throw error if context is not available
            let context = context.inner().as_ref().ok_or("Context not available")?;

            <$request_name as swap::cli::api::request::Request>::request(args, context.clone())
                .await
                .to_string_result()
        }
    };
    ($fn_name:ident, $request_name:ident, no_args) => {
        #[tauri::command]
        async fn $fn_name(
            context: tauri::State<'_, Option<Arc<Context>>>,
        ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
            // Throw error if context is not available
            let context = context.inner().as_ref().ok_or("Context not available")?;

            <$request_name as swap::cli::api::request::Request>::request(
                $request_name {},
                context.clone(),
            )
            .await
            .to_string_result()
        }
    };
}

tauri_command!(get_balance, BalanceArgs);
tauri_command!(buy_xmr, BuyXmrArgs);
tauri_command!(resume_swap, ResumeSwapArgs);
tauri_command!(withdraw_btc, WithdrawBtcArgs);
tauri_command!(suspend_current_swap, SuspendCurrentSwapArgs, no_args);
tauri_command!(get_swap_infos_all, GetSwapInfosAllArgs, no_args);
tauri_command!(get_history, GetHistoryArgs, no_args);

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.app_handle().to_owned().clone();

    app_handle.manage::<Option<Arc<Context>>>(None);

    tauri::async_runtime::spawn(async move {
        let context = ContextBuilder::new(true)
            .with_bitcoin(Bitcoin {
                bitcoin_electrum_rpc_url: None,
                bitcoin_target_block: None,
            })
            .with_monero(Monero {
                monero_daemon_address: None,
            })
            .with_json(true)
            .with_debug(true)
            .with_tauri(TauriHandle::new(app_handle.clone()))
            .build()
            .await
            .expect("failed to create context");
        app_handle.manage::<Option<Arc<Context>>>(Arc::new(context).into());
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            get_balance,
            get_swap_infos_all,
            withdraw_btc,
            buy_xmr,
            resume_swap,
            get_history,
            suspend_current_swap
        ])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::Exit | RunEvent::ExitRequested { .. } => {
                let context = app.state::<Option<Arc<Context>>>().inner().as_ref();

                if let Some(context) = context {
                    if let Err(err) = context.cleanup() {
                        println!("Cleanup failed {}", err);
                    }
                }
            }
            _ => {}
        })
}
