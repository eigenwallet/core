use std::collections::HashMap;
use std::io::Write;
use std::result::Result;
use std::time::Duration;
use swap::cli::{
    api::{
        ContextBuilder, NetworkProxyConfig, data,
        request::{
            BalanceArgs, BuyXmrArgs, CancelAndRefundArgs, ChangeMoneroNodeArgs,
            CheckElectrumNodeArgs, CheckElectrumNodeResponse, CheckMoneroNodeArgs,
            CheckMoneroNodeResponse, CheckSeedArgs, CheckSeedResponse, CreateMoneroSubaddressArgs,
            DeleteAllLogsArgs, DfxAuthenticateResponse, ExportBitcoinWalletArgs,
            GetBitcoinAddressArgs, GetCurrentSwapArgs, GetDataDirArgs, GetHistoryArgs, GetLogsArgs,
            GetMoneroAddressesArgs, GetMoneroBalanceArgs, GetMoneroHistoryArgs,
            GetMoneroMainAddressArgs, GetMoneroSeedArgs, GetMoneroSubaddressesArgs,
            GetMoneroSyncProgressArgs, GetPendingApprovalsResponse, GetRestoreHeightArgs,
            GetSwapInfoArgs, GetSwapInfosAllArgs, GetSwapTimelockArgs, MoneroRecoveryArgs,
            RedactArgs, RefreshP2PArgs, RejectApprovalArgs, RejectApprovalResponse,
            ResolveApprovalArgs, ResumeSwapArgs, SendMoneroArgs, SetMoneroSubaddressLabelArgs,
            SetMoneroWalletPasswordArgs, SetRestoreHeightArgs, SuspendCurrentSwapArgs,
            WithdrawBtcArgs,
        },
        tauri_bindings::{ContextStatus, NetworkProxy, TauriSettings},
    },
    command::Bitcoin,
};
use swap_p2p::libp2p_ext::MultiAddrVecExt;
use tauri_plugin_dialog::DialogExt;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{State, commands::util::ToStringResult};

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
            get_swap_timelock,
            withdraw_btc,
            buy_xmr,
            resume_swap,
            get_history,
            monero_recovery,
            get_logs,
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
            delete_all_logs,
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
            set_monero_wallet_password,
            dfx_authenticate,
            change_monero_node,
            get_context_status,
            get_monero_subaddresses,
            create_monero_subaddress,
            set_monero_subaddress_label,
            refresh_p2p,
            http_get,
            http_post_json,
            check_socks5_address,
            get_updater_proxy_url
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpGetArgs {
    pub url: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpPostJsonArgs {
    pub url: String,
    pub body: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[tauri::command]
pub async fn http_get(args: HttpGetArgs) -> Result<HttpResponse, String> {
    send_http_request(reqwest::Method::GET, args.url, None).await
}

#[tauri::command]
pub async fn http_post_json(args: HttpPostJsonArgs) -> Result<HttpResponse, String> {
    send_http_request(reqwest::Method::POST, args.url, Some(args.body)).await
}

#[tauri::command]
pub async fn check_socks5_address(address: String) -> bool {
    tokio::task::spawn_blocking(move || tor_socks5::probe_addr_str(&address))
        .await
        .unwrap_or(false)
}

/// Build the updater SOCKS5 URL from a persisted IPv4 address.
#[tauri::command]
pub fn get_updater_proxy_url(address: String) -> Result<String, String> {
    let addr: std::net::SocketAddrV4 = address.parse().map_err(|e| {
        format!("Invalid SOCKS5 proxy address '{address}': {e}. Expected IPv4 ip:port, e.g. 127.0.0.1:9050.")
    })?;
    Ok(tor_socks5::Subsystem::Updater.proxy_url_for(addr))
}

async fn send_http_request(
    method: reqwest::Method,
    url: String,
    body: Option<String>,
) -> Result<HttpResponse, String> {
    let parsed_url =
        reqwest::Url::parse(&url).map_err(|e| format!("Failed to parse URL '{url}': {e}"))?;
    let client = crate::http_client::build_http_client(&parsed_url, Duration::from_secs(20))
        .map_err(|e| format!("Failed to build HTTP client for '{url}': {e:#}"))?;

    let mut request = client.request(method.clone(), parsed_url.clone());

    if let Some(body) = body {
        request = request
            .header("Content-Type", "application/json")
            .body(body);
    }

    let response = request.send().await.map_err(|e| {
        format!(
            "Failed to send {} request to '{}': {e:#}",
            method.as_str(),
            parsed_url
        )
    })?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| {
        format!(
            "Failed to read {} response body from '{}': {e:#}",
            method.as_str(),
            parsed_url
        )
    })?;

    Ok(HttpResponse { status, body })
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

    // Parse rendeuvous points
    let rendezvous_points = settings.rendezvous_points.extract_peer_addresses();

    let network_proxy = match settings.network_proxy {
        NetworkProxy::InternalTor => NetworkProxyConfig::InternalTor,
        NetworkProxy::None => NetworkProxyConfig::None,
        NetworkProxy::SystemTorSocks5 { address } => {
            let addr: std::net::SocketAddrV4 = address.parse().map_err(|e| {
                format!("Invalid SOCKS5 proxy address '{address}': {e}. Expected IPv4 ip:port, e.g. 127.0.0.1:9050.")
            })?;
            NetworkProxyConfig::SystemTorSocks5(addr)
        }
    };

    // Store the DFX kill switch from persisted settings.
    state
        .allow_dfx_clearnet
        .store(settings.allow_dfx_clearnet, std::sync::atomic::Ordering::Relaxed);

    // Now populate the context in the background
    let context_result = ContextBuilder::new(testnet)
        .with_bitcoin(Bitcoin {
            bitcoin_electrum_rpc_urls: settings.electrum_rpc_urls.clone(),
            bitcoin_target_block: None,
        })
        .with_monero(settings.monero_node_config)
        .with_json(false)
        .with_network_proxy(network_proxy)
        .with_enable_monero_tor(settings.enable_monero_tor)
        .with_rendezvous_points(rendezvous_points)
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

#[tauri::command(rename = "deleteAllLogs")]
pub async fn delete_all_logs(args: DeleteAllLogsArgs) -> Result<(), String> {
    let data_dir = data::data_dir_from(None, args.is_testnet).to_string_result()?;
    let logs_dir = data_dir.join("logs");

    if !logs_dir.exists() {
        tracing::info!(
            logs_dir = %logs_dir.display(),
            "Log directory does not exist; nothing to clear"
        );
        return Ok(());
    }

    let delete_result: Result<(), String> = async {
        let mut entries = tokio::fs::read_dir(&logs_dir).await.to_string_result()?;
        while let Some(entry) = entries.next_entry().await.to_string_result()? {
            let path = entry.path();
            let file_type = entry.file_type().await.to_string_result()?;

            if file_type.is_dir() {
                tokio::fs::remove_dir_all(&path).await.to_string_result()?;
            } else {
                tokio::fs::remove_file(&path).await.to_string_result()?;
            }
        }
        Ok(())
    }
    .await;

    match delete_result {
        Ok(()) => {
            tracing::info!(logs_dir = %logs_dir.display(), "Cleared all log files");
            Ok(())
        }
        Err(err) => {
            tracing::error!(
                logs_dir = %logs_dir.display(),
                error = %err,
                "Failed to clear log files"
            );
            Err(err)
        }
    }
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
    const DFX_API_BASE_URL: &str = "https://api.dfx.swiss";
    use dfx_swiss_sdk::{DfxClient, SignRequest};
    use tokio::sync::{mpsc, oneshot};
    use tokio_util::task::AbortOnDropHandle;

    // DFX is only available when the user enables the clearnet path.
    if !state
        .allow_dfx_clearnet
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err(
            "DFX integration is disabled. Enable 'DFX (clearnet only)' in Settings to use it."
                .to_string(),
        );
    }

    let context = state.context();

    // Get the monero wallet manager
    let monero_manager = context
        .try_get_monero_manager()
        .await
        .map_err(|_| "Monero wallet manager not available for DFX authentication".to_string())?;

    let wallet = monero_manager.main_wallet().await;
    let address = wallet
        .main_address()
        .await
        .map_err(|e| e.to_string())?
        .to_string();

    // Create channel for authentication
    let (auth_tx, mut auth_rx) = mpsc::channel::<(SignRequest, oneshot::Sender<String>)>(10);

    // Keep DFX on its direct HTTP path.
    let mut client = DfxClient::new(address, Some(DFX_API_BASE_URL.to_string()), auth_tx);

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
tauri_command!(get_swap_timelock, GetSwapTimelockArgs);
tauri_command!(get_history, GetHistoryArgs, no_args);
tauri_command!(get_monero_addresses, GetMoneroAddressesArgs, no_args);
tauri_command!(get_monero_history, GetMoneroHistoryArgs, no_args);
tauri_command!(get_current_swap, GetCurrentSwapArgs, no_args);
tauri_command!(set_monero_restore_height, SetRestoreHeightArgs);
tauri_command!(get_restore_height, GetRestoreHeightArgs, no_args);
tauri_command!(set_monero_wallet_password, SetMoneroWalletPasswordArgs);
tauri_command!(get_monero_main_address, GetMoneroMainAddressArgs, no_args);
tauri_command!(get_monero_balance, GetMoneroBalanceArgs, no_args);
tauri_command!(get_monero_sync_progress, GetMoneroSyncProgressArgs, no_args);
tauri_command!(get_monero_subaddresses, GetMoneroSubaddressesArgs);
tauri_command!(create_monero_subaddress, CreateMoneroSubaddressArgs);
tauri_command!(set_monero_subaddress_label, SetMoneroSubaddressLabelArgs);
tauri_command!(get_monero_seed, GetMoneroSeedArgs, no_args);
tauri_command!(refresh_p2p, RefreshP2PArgs, no_args);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_updater_proxy_url_accepts_valid_ipv4() {
        // Keep the updater URL format aligned with `ProxyConfig::url()`.
        let url = get_updater_proxy_url("127.0.0.1:9050".to_string()).unwrap();
        assert_eq!(url, "socks5h://updater:updater@127.0.0.1:9050");
    }

    #[test]
    fn get_updater_proxy_url_rejects_invalid_address() {
        // Reject non-IPv4 `ip:port` inputs.
        assert!(get_updater_proxy_url("localhost:9050".to_string()).is_err());
        assert!(get_updater_proxy_url("[::1]:9050".to_string()).is_err());
        assert!(get_updater_proxy_url("127.0.0.1".to_string()).is_err());
        assert!(get_updater_proxy_url("".to_string()).is_err());
    }

    #[tokio::test]
    async fn check_socks5_address_returns_false_for_invalid_input() {
        // Invalid input should map to `false`.
        assert!(!check_socks5_address("not-an-addr".to_string()).await);
        assert!(!check_socks5_address("".to_string()).await);
        assert!(!check_socks5_address("localhost:9050".to_string()).await);
    }
}
