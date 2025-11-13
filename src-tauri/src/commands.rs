use std::collections::HashMap;
use std::io::Write;
use std::result::Result;
use swap::cli::{
    api::{
        data,
        request::{
            BalanceArgs, BuyXmrArgs, CancelAndRefundArgs, ChangeMoneroNodeArgs,
            CheckElectrumNodeArgs, CheckElectrumNodeResponse, CheckMoneroNodeArgs,
            CheckMoneroNodeResponse, CheckSeedArgs, CheckSeedResponse, DfxAuthenticateResponse,
            ExportBitcoinWalletArgs, GetBitcoinAddressArgs, GetCurrentSwapArgs, GetDataDirArgs,
            GetHistoryArgs, GetLogsArgs, GetMoneroAddressesArgs, GetMoneroBalanceArgs,
            GetMoneroHistoryArgs, GetMoneroMainAddressArgs, GetMoneroSeedArgs,
            GetMoneroSubaddressesArgs, GetMoneroSyncProgressArgs, GetPendingApprovalsResponse,
            GetRestoreHeightArgs, GetSwapInfoArgs, GetSwapInfosAllArgs, ListSellersArgs,
            MoneroRecoveryArgs, RedactArgs, RejectApprovalArgs, RejectApprovalResponse,
            ResolveApprovalArgs, ResumeSwapArgs, SendMoneroArgs, SetRestoreHeightArgs,
            SuspendCurrentSwapArgs, WithdrawBtcArgs, CreateMoneroSubaddressArgs,
            SetMoneroSubaddressLabelArgs,
        },
        tauri_bindings::{ContextStatus, TauriSettings},
        ContextBuilder,
    },
    command::Bitcoin,
};
use tauri_plugin_dialog::DialogExt;
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::{commands::util::ToStringResult, State};

/// This macro returns the list of all command handlers
/// You can call this and insert the output into [`tauri::app::Builder::invoke_handler`]
///
/// Note: When you add a new command, add it here.
#[macro_export]
macro_rules! generate_command_handlers {
    () => {
        tauri::generate_handler![
            get_balance,
            get_bitcoin_address,
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
            get_context_status,
            get_monero_subaddresses,
            create_monero_subaddress,
            set_monero_subaddress_label
        ]
    };
}

#[macro_use]
mod util {
    use std::result::Result;

    /// Trait to convert Result<T, E> to Result<T, String>
    /// Tauri commands require the error type to be a string
    pub(crate) trait ToStringResult<T> {
        fn to_string_result(self) -> Result<T, String>;
    }

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
    /// ```
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
            pub async fn $fn_name(
                state: tauri::State<'_, State>,
                args: $request_name,
            ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
                <$request_name as swap::cli::api::request::Request>::request(args, state.context())
                    .await
                    .to_string_result()
            }
        };
        ($fn_name:ident, $request_name:ident, no_args) => {
            #[tauri::command]
            pub async fn $fn_name(
                state: tauri::State<'_, State>,
            ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
                <$request_name as swap::cli::api::request::Request>::request(
                    $request_name {},
                    state.context(),
                )
                .await
                .to_string_result()
            }
        };
    }
}

/// Tauri command to initialize the Context
#[tauri::command]
pub async fn initialize_context(
    settings: TauriSettings,
    testnet: bool,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    // We want to prevent multiple initalizations at the same time
    let _context_lock = state
        .context_lock
        .try_lock()
        .map_err(|_| "Context is already being initialized".to_string())?;

    // Fail if the context is already initialized
    // TODO: Maybe skip the stuff below if one of the context fields is already initialized?
    // if context_lock.is_some() {
    //     return Err("Context is already initialized".to_string());
    // }

    // Get tauri handle from the state
    let tauri_handle = state.handle.clone();

    // Now populate the context in the background
    let context_result = ContextBuilder::new(testnet)
        .with_bitcoin(Bitcoin {
            bitcoin_electrum_rpc_urls: settings.electrum_rpc_urls.clone(),
            bitcoin_target_block: None,
        })
        .with_monero(settings.monero_node_config)
        .with_json(false)
        .with_tor(settings.use_tor)
        .with_enable_monero_tor(settings.enable_monero_tor)
        .with_tauri(tauri_handle.clone())
        .build(state.context())
        .await;

    match context_result {
        Ok(()) => {
            tracing::info!("Context initialized");
            Ok(())
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to initialize context");
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn get_context_status(state: tauri::State<'_, State>) -> Result<ContextStatus, String> {
    Ok(state.context().status().await)
}

#[tauri::command]
pub async fn resolve_approval_request(
    args: ResolveApprovalArgs,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let request_id = args
        .request_id
        .parse()
        .map_err(|e| format!("Invalid request ID '{}': {}", args.request_id, e))?;

    state
        .handle
        .resolve_approval(request_id, args.accept)
        .await
        .to_string_result()?;

    Ok(())
}

#[tauri::command]
pub async fn reject_approval_request(
    args: RejectApprovalArgs,
    state: tauri::State<'_, State>,
) -> Result<RejectApprovalResponse, String> {
    let request_id = args
        .request_id
        .parse()
        .map_err(|e| format!("Invalid request ID '{}': {}", args.request_id, e))?;

    state
        .handle
        .reject_approval(request_id)
        .await
        .to_string_result()?;

    Ok(RejectApprovalResponse { success: true })
}

#[tauri::command]
pub async fn get_pending_approvals(
    state: tauri::State<'_, State>,
) -> Result<GetPendingApprovalsResponse, String> {
    let approvals = state
        .handle
        .get_pending_approvals()
        .await
        .to_string_result()?;

    Ok(GetPendingApprovalsResponse { approvals })
}

#[tauri::command]
pub async fn check_monero_node(
    args: CheckMoneroNodeArgs,
    _: tauri::State<'_, State>,
) -> Result<CheckMoneroNodeResponse, String> {
    args.request().await.to_string_result()
}

#[tauri::command]
pub async fn check_electrum_node(
    args: CheckElectrumNodeArgs,
    _: tauri::State<'_, State>,
) -> Result<CheckElectrumNodeResponse, String> {
    args.request().await.to_string_result()
}

#[tauri::command]
pub async fn check_seed(
    args: CheckSeedArgs,
    _: tauri::State<'_, State>,
) -> Result<CheckSeedResponse, String> {
    args.request().await.to_string_result()
}

// Returns the data directory
// This is independent of the context to ensure the user can open the directory even if the context cannot
// be initialized (for troubleshooting purposes)
#[tauri::command]
pub async fn get_data_dir(
    args: GetDataDirArgs,
    _: tauri::State<'_, State>,
) -> Result<String, String> {
    Ok(data::data_dir_from(None, args.is_testnet)
        .to_string_result()?
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
pub async fn save_txt_files(
    app: tauri::AppHandle,
    zip_file_name: String,
    content: HashMap<String, String>,
) -> Result<(), String> {
    // Step 1: Get the owned PathBuf from the dialog
    let path_buf_from_dialog: tauri_plugin_dialog::FilePath = app
        .dialog()
        .file()
        .set_file_name(format!("{}.zip", &zip_file_name).as_str())
        .add_filter(&zip_file_name, &["zip"])
        .blocking_save_file() // This returns Option<PathBuf>
        .ok_or_else(|| "Dialog cancelled or file path not selected".to_string())?; // Converts to Result<PathBuf, String> and unwraps to PathBuf

    // Step 2: Now get a &Path reference from the owned PathBuf.
    // The user's code structure implied an .as_path().ok_or_else(...) chain which was incorrect for &Path.
    // We'll directly use the PathBuf, or if &Path is strictly needed:
    let selected_file_path: &std::path::Path = path_buf_from_dialog
        .as_path()
        .ok_or_else(|| "Could not convert file path".to_string())?;

    let zip_file = std::fs::File::create(selected_file_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut zip = ZipWriter::new(zip_file);

    for (filename, file_content_str) in content.iter() {
        zip.start_file(
            format!("{}.txt", filename).as_str(),
            SimpleFileOptions::default(),
        ) // Pass &str to start_file
        .map_err(|e| format!("Failed to start file {}: {}", &filename, e))?; // Use &filename

        zip.write_all(file_content_str.as_bytes())
            .map_err(|e| format!("Failed to write to file {}: {}", &filename, e))?;
        // Use &filename
    }

    zip.finish()
        .map_err(|e| format!("Failed to finish zip: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn dfx_authenticate(
    state: tauri::State<'_, State>,
) -> Result<DfxAuthenticateResponse, String> {
    use dfx_swiss_sdk::{DfxClient, SignRequest};
    use tokio::sync::{mpsc, oneshot};
    use tokio_util::task::AbortOnDropHandle;

    let context = state.context();

    // Get the monero wallet manager
    let monero_manager = context
        .try_get_monero_manager()
        .await
        .map_err(|_| "Monero wallet manager not available for DFX authentication".to_string())?;

    let wallet = monero_manager.main_wallet().await;
    let address = wallet.main_address().await.to_string();

    // Create channel for authentication
    let (auth_tx, mut auth_rx) = mpsc::channel::<(SignRequest, oneshot::Sender<String>)>(10);

    // Create DFX client
    let mut client = DfxClient::new(address, Some("https://api.dfx.swiss".to_string()), auth_tx);

    // Start signing task with AbortOnDropHandle
    let signing_task = tokio::spawn(async move {
        tracing::info!("DFX signing service started and listening for requests");

        while let Some((sign_request, response_tx)) = auth_rx.recv().await {
            tracing::debug!(
                message = %sign_request.message,
                blockchains = ?sign_request.blockchains,
                "Received DFX signing request"
            );

            // Sign the message using the main Monero wallet
            let signature = match wallet
                .sign_message(&sign_request.message, None, false)
                .await
            {
                Ok(sig) => {
                    tracing::debug!(
                        signature_preview = %&sig[..std::cmp::min(50, sig.len())],
                        "Message signed successfully for DFX"
                    );
                    sig
                }
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to sign message for DFX");
                    continue;
                }
            };

            // Send signature back to DFX client
            if let Err(_) = response_tx.send(signature) {
                tracing::warn!("Failed to send signature response through channel to DFX client");
            }
        }

        tracing::info!("DFX signing service stopped");
    });

    // Create AbortOnDropHandle so the task gets cleaned up
    let _abort_handle = AbortOnDropHandle::new(signing_task);

    // Authenticate with DFX
    tracing::info!("Starting DFX authentication...");
    client
        .authenticate()
        .await
        .map_err(|e| format!("Failed to authenticate with DFX: {}", e))?;

    let access_token = client
        .access_token
        .as_ref()
        .ok_or("No access token available after authentication")?
        .clone();

    let kyc_url = format!("https://app.dfx.swiss/buy?session={}", access_token);

    tracing::info!("DFX authentication completed successfully");

    Ok(DfxAuthenticateResponse {
        access_token,
        kyc_url,
    })
}

// Here we define the Tauri commands that will be available to the frontend
// The commands are defined using the `tauri_command!` macro.
// Implementations are handled by the Request trait
tauri_command!(get_balance, BalanceArgs);
tauri_command!(buy_xmr, BuyXmrArgs);
tauri_command!(resume_swap, ResumeSwapArgs);
tauri_command!(withdraw_btc, WithdrawBtcArgs);
tauri_command!(monero_recovery, MoneroRecoveryArgs);
tauri_command!(get_logs, GetLogsArgs);
tauri_command!(list_sellers, ListSellersArgs);
tauri_command!(cancel_and_refund, CancelAndRefundArgs);
tauri_command!(redact, RedactArgs);
tauri_command!(send_monero, SendMoneroArgs);
tauri_command!(change_monero_node, ChangeMoneroNodeArgs);

// These commands require no arguments
tauri_command!(get_bitcoin_address, GetBitcoinAddressArgs, no_args);
tauri_command!(get_wallet_descriptor, ExportBitcoinWalletArgs, no_args);
tauri_command!(suspend_current_swap, SuspendCurrentSwapArgs, no_args);
tauri_command!(get_swap_info, GetSwapInfoArgs);
tauri_command!(get_swap_infos_all, GetSwapInfosAllArgs, no_args);
tauri_command!(get_history, GetHistoryArgs, no_args);
tauri_command!(get_monero_addresses, GetMoneroAddressesArgs, no_args);
tauri_command!(get_monero_history, GetMoneroHistoryArgs, no_args);
tauri_command!(get_current_swap, GetCurrentSwapArgs, no_args);
tauri_command!(set_monero_restore_height, SetRestoreHeightArgs);
tauri_command!(get_restore_height, GetRestoreHeightArgs, no_args);
tauri_command!(get_monero_main_address, GetMoneroMainAddressArgs, no_args);
tauri_command!(get_monero_balance, GetMoneroBalanceArgs, no_args);
tauri_command!(get_monero_sync_progress, GetMoneroSyncProgressArgs, no_args);
tauri_command!(get_monero_subaddresses, GetMoneroSubaddressesArgs);
tauri_command!(create_monero_subaddress, CreateMoneroSubaddressArgs);
tauri_command!(set_monero_subaddress_label, SetMoneroSubaddressLabelArgs);
tauri_command!(get_monero_seed, GetMoneroSeedArgs, no_args);
