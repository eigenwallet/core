import { invoke as invokeUnsafe } from "@tauri-apps/api/core";
import {
  BalanceArgs,
  BalanceResponse,
  BuyXmrArgs,
  GetLogsArgs,
  GetLogsResponse,
  GetSwapInfoResponse,
  MoneroRecoveryArgs,
  ResumeSwapArgs,
  ResumeSwapResponse,
  SuspendCurrentSwapResponse,
  WithdrawBtcArgs,
  WithdrawBtcResponse,
  GetSwapInfoArgs,
  ExportBitcoinWalletResponse,
  GetBitcoinAddressResponse,
  CheckMoneroNodeArgs,
  CheckSeedArgs,
  CheckSeedResponse,
  CheckMoneroNodeResponse,
  TauriSettings,
  CheckElectrumNodeArgs,
  CheckElectrumNodeResponse,
  GetMoneroAddressesResponse,
  GetDataDirArgs,
  ResolveApprovalArgs,
  ResolveApprovalResponse,
  RedactArgs,
  RedactResponse,
  GetCurrentSwapResponse,
  LabeledMoneroAddress,
  GetMoneroHistoryResponse,
  GetMoneroMainAddressResponse,
  SubaddressSummary,
  GetMoneroSubaddressesArgs,
  GetMoneroSubaddressesResponse,
  CreateMoneroSubaddressArgs,
  CreateMoneroSubaddressResponse,
  SetMoneroSubaddressLabelArgs,
  SetMoneroSubaddressLabelResponse,
  GetMoneroBalanceResponse,
  SendMoneroArgs,
  SendMoneroResponse,
  GetMoneroSyncProgressResponse,
  GetPendingApprovalsResponse,
  DfxAuthenticateResponse,
  RejectApprovalArgs,
  RejectApprovalResponse,
  SetRestoreHeightArgs,
  SetRestoreHeightResponse,
  GetRestoreHeightResponse,
  MoneroNodeConfig,
  GetMoneroSeedResponse,
  ContextStatus,
  GetSwapTimelockArgs,
  GetSwapTimelockResponse,
  SetMoneroWalletPasswordResponse,
  SetMoneroWalletPasswordArgs,
} from "models/tauriModel";
import {
  rpcSetSwapInfo,
  rpcSetSwapInfosLoaded,
  approvalRequestsReplaced,
  timelockChangeEventReceived,
} from "store/features/rpcSlice";
import { selectAllSwapIds } from "store/selectors";
import { setBitcoinBalance } from "store/features/bitcoinWalletSlice";
import {
  setMainAddress,
  setBalance,
  setSyncProgress,
  setHistory,
  setRestoreHeight,
  setSubaddresses,
} from "store/features/walletSlice";
import { store } from "./store/storeRenderer";
import { MoneroRecoveryResponse } from "models/rpcModel";
import logger from "utils/logger";
import { getNetwork, isTestnet } from "store/config";
import { Blockchain, Network } from "store/types";
import { setStatus } from "store/features/nodesSlice";
import { CliLog } from "models/cliModel";
import { logsToRawString, parseLogsFromString } from "utils/parseUtils";
import { DEFAULT_RENDEZVOUS_POINTS } from "store/defaults";

/// These are the official donation address for the eigenwallet/core project
const DONATION_ADDRESS_MAINNET =
  "4A1tNBcsxhQA7NkswREXTD1QGz8mRyA7fGnCzPyTwqzKdDFMNje7iHUbGhCetfVUZa1PTuZCoPKj8gnJuRrFYJ2R2CEzqbJ";
const DONATION_ADDRESS_STAGENET =
  "56E274CJxTyVuuFG651dLURKyneoJ5LsSA5jMq4By9z9GBNYQKG8y5ejTYkcvZxarZW6if14ve8xXav2byK4aRnvNdKyVxp";

/// Signature by binarybaron for the donation address
/// https://github.com/binarybaron/
///
/// Get the key from:
/// - https://github.com/eigenwallet/core/blob/master/utils/gpg_keys/binarybaron.asc
/// - https://unstoppableswap.net/binarybaron.asc
const DONATION_ADDRESS_MAINNET_SIG = `
-----BEGIN PGP SIGNED MESSAGE-----
Hash: SHA256

4A1tNBcsxhQA7NkswREXTD1QGz8mRyA7fGnCzPyTwqzKdDFMNje7iHUbGhCetfVUZa1PTuZCoPKj8gnJuRrFYJ2R2CEzqbJ is our donation address (signed by binarybaron)
-----BEGIN PGP SIGNATURE-----

iQGzBAEBCAAdFiEEBRhGD+vsHaFKFVp7RK5vCxZqrVoFAmjxV4YACgkQRK5vCxZq
rVrFogv9F650Um1TsPlqQ+7kdobCwa7yH5uXOp1p22YaiwWGHKRU5rUSb6Ac+zI0
3Io39VEoZufQqXqEqaiH7Q/08ABQR5r0TTPtSLNjOSEQ+ecClwv7MeF5CIXZYDdB
AlEOnlL0CPfA24GQMhfp9lvjNiTBA2NikLARWJrc1JsLrFMK5rHesv7VHJEtm/gu
We5eAuNOM2k3nAABTWzLiMJkH+G1amJmfkCKkBCk04inA6kZ5COUikMupyQDtsE4
hrr/KrskMuXzGY+rjP6NhWqr/twKj819TrOxlYD4vK68cZP+jx9m+vSBE6mxgMbN
tBVdo9xFVCVymOYQCV8BRY8ScqP+YPNV5d6BMyDH9tvHJrGqZTNQiFhVX03Tw6mg
hccEqYP1J/TaAlFg/P4HtqsxPBZD6x3IdSxXhrJ0IjrqLpVtKyQlTZGsJuNjFWG8
LKixaxxR7iWsyRZVCnEqCgDN8hzKZIE3Ph+kLTa4z4mTNEYyWUNeKRrFrSxKvEOK
KM0Pp53f
=O/zf
-----END PGP SIGNATURE-----
`;

async function invoke<ARGS, RESPONSE>(
  command: string,
  args: ARGS,
): Promise<RESPONSE> {
  return invokeUnsafe(command, {
    args: args as Record<string, unknown>,
  }) as Promise<RESPONSE>;
}

async function invokeNoArgs<RESPONSE>(command: string): Promise<RESPONSE> {
  return invokeUnsafe(command) as Promise<RESPONSE>;
}

export async function checkBitcoinBalance() {
  // If we are already syncing, don't start a new sync
  if (
    Object.values(store.getState().rpc?.state.background ?? {}).some(
      (progress) =>
        progress.componentName === "SyncingBitcoinWallet" &&
        progress.progress.type === "Pending",
    )
  ) {
    console.log(
      "checkBitcoinBalance() was called but we are already syncing Bitcoin, skipping",
    );
    return;
  }

  const response = await invoke<BalanceArgs, BalanceResponse>("get_balance", {
    force_refresh: true,
  });

  store.dispatch(setBitcoinBalance(response.balance));
}

export async function buyXmr() {
  const state = store.getState();

  // Determine based on redeem and refund policy which addresses to pass in
  //
  // null means internal wallet
  const bitcoinChangeAddress =
    state.settings.bitcoinRefundPolicy === "external"
      ? state.settings.externalBitcoinRefundAddress
      : null;
  const moneroReceiveAddress =
    state.settings.moneroRedeemPolicy === "external"
      ? state.settings.externalMoneroRedeemAddress
      : null;

  const donationPercentage = state.settings.donateToDevelopment;

  const address_pool: LabeledMoneroAddress[] = [];
  if (donationPercentage !== false && donationPercentage > 0) {
    const donation_address = isTestnet()
      ? DONATION_ADDRESS_STAGENET
      : DONATION_ADDRESS_MAINNET;

    address_pool.push(
      {
        // We need to assert this as being not null even though it can be null
        //
        // This is correct because a LabeledMoneroAddress can actually have a null address but
        // typeshare cannot express that yet (easily)
        //
        // TODO: Let typescript do its job here and not assert it
        address: moneroReceiveAddress!,
        percentage: 1 - donationPercentage,
        label: "Your wallet",
      },
      {
        address: donation_address,
        percentage: donationPercentage,
        label: "Tip to the developers",
      },
    );
  } else {
    address_pool.push({
      // We need to assert this as being not null even though it can be null
      //
      // This is correct because a LabeledMoneroAddress can actually have a null address but
      // typeshare cannot express that yet (easily)
      //
      // TODO: Let typescript do its job here and not assert it
      address: moneroReceiveAddress!,
      percentage: 1,
      label: "Your wallet",
    });
  }

  await invoke<BuyXmrArgs, void>("buy_xmr", {
    monero_receive_pool: address_pool,
    // We convert null to undefined because typescript
    // expects undefined if the field is optional and does not accept null here
    bitcoin_change_address: bitcoinChangeAddress ?? undefined,
  });
}

export async function initializeContext() {
  const network = getNetwork();
  const testnet = isTestnet();
  const useTor = store.getState().settings.enableTor;

  // Get all Bitcoin nodes without checking availability
  // The backend ElectrumBalancer will handle load balancing and failover
  const bitcoinNodes =
    store.getState().settings.nodes[network][Blockchain.Bitcoin];

  // For Monero nodes, determine whether to use pool or custom node
  const useMoneroRpcPool = store.getState().settings.useMoneroRpcPool;

  const useMoneroTor = store.getState().settings.enableMoneroTor;
  const rendezvousPoints = Array.from(
    new Set([
      ...store.getState().settings.rendezvousPoints,
      ...DEFAULT_RENDEZVOUS_POINTS,
    ]),
  );

  const moneroNodeUrl =
    store.getState().settings.nodes[network][Blockchain.Monero][0] ?? null;

  // Check the state of the Monero node
  const moneroNodeConfig =
    useMoneroRpcPool ||
    moneroNodeUrl == null ||
    !(await getMoneroNodeStatus(moneroNodeUrl, network))
      ? { type: "Pool" as const }
      : {
          type: "SingleNode" as const,
          content: {
            url: moneroNodeUrl,
          },
        };

  // Initialize Tauri settings
  const tauriSettings: TauriSettings = {
    electrum_rpc_urls: bitcoinNodes,
    monero_node_config: moneroNodeConfig,
    use_tor: useTor,
    enable_monero_tor: useMoneroTor,
    rendezvous_points: rendezvousPoints,
  };

  logger.info({ tauriSettings }, "Initializing context with settings");

  try {
    await invokeUnsafe<void>("initialize_context", {
      settings: tauriSettings,
      testnet,
    });
    logger.info("Initialized context");
  } catch (error) {
    throw new Error(String(error));
  }
}

export async function updateAllNodeStatuses() {
  const network = getNetwork();
  const settings = store.getState().settings;

  // We pass all electrum servers to the backend without checking them (ElectrumBalancer handles failover),
  // but check these anyway since the status appears in the GUI.
  // Only check Monero nodes if we're using custom nodes (not RPC pool).
  await Promise.all(
    (settings.useMoneroRpcPool
      ? [Blockchain.Bitcoin]
      : [Blockchain.Bitcoin, Blockchain.Monero]
    )
      .map((blockchain) =>
        settings.nodes[network][blockchain].map((node) =>
          updateNodeStatus(node, blockchain, network),
        ),
      )
      .flat(),
  );
}

export async function cheapCheckBitcoinBalance() {
  const response = await invoke<BalanceArgs, BalanceResponse>("get_balance", {
    force_refresh: false,
  });

  store.dispatch(setBitcoinBalance(response.balance));
}

export async function getBitcoinAddress() {
  const response = await invokeNoArgs<GetBitcoinAddressResponse>(
    "get_bitcoin_address",
  );

  return response.address;
}

export async function getAllSwapInfos() {
  const response =
    await invokeNoArgs<GetSwapInfoResponse[]>("get_swap_infos_all");

  response.forEach((swapInfo) => {
    store.dispatch(rpcSetSwapInfo(swapInfo));
  });

  store.dispatch(rpcSetSwapInfosLoaded());
}

export async function getSwapInfo(swapId: string) {
  const response = await invoke<GetSwapInfoArgs, GetSwapInfoResponse>(
    "get_swap_info",
    {
      swap_id: swapId,
    },
  );

  store.dispatch(rpcSetSwapInfo(response));
}

export async function getSwapTimelock(swapId: string) {
  const response = await invoke<GetSwapTimelockArgs, GetSwapTimelockResponse>(
    "get_swap_timelock",
    {
      swap_id: swapId,
    },
  );

  store.dispatch(
    timelockChangeEventReceived({
      swap_id: response.swap_id,
      timelock: response.timelock,
    }),
  );
}

export async function getAllSwapTimelocks() {
  const swapIds = selectAllSwapIds(store.getState());

  await Promise.all(
    swapIds.map(async (swapId) => {
      try {
        await getSwapTimelock(swapId);
      } catch (error) {
        logger.debug(`Failed to fetch timelock for swap ${swapId}: ${error}`);
      }
    }),
  );
}

export async function sweepBtc(address: string): Promise<string> {
  const response = await invoke<WithdrawBtcArgs, WithdrawBtcResponse>(
    "withdraw_btc",
    {
      address,
      amount: undefined,
    },
  );

  // We check the balance, this is cheap and does not sync the wallet
  // but instead uses our local cached balance
  await cheapCheckBitcoinBalance();

  return response.txid;
}

export async function resumeSwap(swapId: string) {
  await invoke<ResumeSwapArgs, ResumeSwapResponse>("resume_swap", {
    swap_id: swapId,
  });
}

export async function suspendCurrentSwap() {
  await invokeNoArgs<SuspendCurrentSwapResponse>("suspend_current_swap");
}

export async function getCurrentSwapId() {
  return await invokeNoArgs<GetCurrentSwapResponse>("get_current_swap");
}

export async function getMoneroRecoveryKeys(
  swapId: string,
): Promise<MoneroRecoveryResponse> {
  return await invoke<MoneroRecoveryArgs, MoneroRecoveryResponse>(
    "monero_recovery",
    {
      swap_id: swapId,
    },
  );
}

export async function checkContextStatus(): Promise<ContextStatus> {
  return await invokeNoArgs<ContextStatus>("get_context_status");
}

export async function getLogsOfSwap(
  swapId: string,
  redact: boolean,
): Promise<GetLogsResponse> {
  return await invoke<GetLogsArgs, GetLogsResponse>("get_logs", {
    swap_id: swapId,
    redact,
  });
}

/// Call the rust backend to redact logs.
export async function redactLogs(
  logs: (string | CliLog)[],
): Promise<(string | CliLog)[]> {
  const response = await invoke<RedactArgs, RedactResponse>("redact", {
    text: logsToRawString(logs),
  });

  return parseLogsFromString(response.text);
}

export async function getWalletDescriptor() {
  return await invokeNoArgs<ExportBitcoinWalletResponse>(
    "get_wallet_descriptor",
  );
}

export async function getMoneroNodeStatus(
  node: string,
  network: Network,
): Promise<boolean> {
  const response = await invoke<CheckMoneroNodeArgs, CheckMoneroNodeResponse>(
    "check_monero_node",
    {
      url: node,
      network,
    },
  );

  return response.available;
}

export async function getElectrumNodeStatus(url: string): Promise<boolean> {
  const response = await invoke<
    CheckElectrumNodeArgs,
    CheckElectrumNodeResponse
  >("check_electrum_node", {
    url,
  });

  return response.available;
}

export async function getNodeStatus(
  url: string,
  blockchain: Blockchain,
  network: Network,
): Promise<boolean> {
  switch (blockchain) {
    case Blockchain.Monero:
      return await getMoneroNodeStatus(url, network);
    case Blockchain.Bitcoin:
      return await getElectrumNodeStatus(url);
    default:
      throw new Error(`Unsupported blockchain: ${blockchain}`);
  }
}

async function updateNodeStatus(
  node: string,
  blockchain: Blockchain,
  network: Network,
) {
  const status = await getNodeStatus(node, blockchain, network);

  store.dispatch(setStatus({ node, status, blockchain }));
}

export async function getMoneroAddresses(): Promise<GetMoneroAddressesResponse> {
  return await invokeNoArgs<GetMoneroAddressesResponse>("get_monero_addresses");
}

export async function getRestoreHeight(): Promise<GetRestoreHeightResponse> {
  const restoreHeight =
    await invokeNoArgs<GetRestoreHeightResponse>("get_restore_height");
  store.dispatch(setRestoreHeight(restoreHeight));
  return restoreHeight;
}

export async function setMoneroRestoreHeight(
  height: number | Date,
): Promise<SetRestoreHeightResponse> {
  const args: SetRestoreHeightArgs =
    typeof height === "number"
      ? { type: "Height", height: height }
      : {
          type: "Date",
          height: {
            year: height.getFullYear(),
            month: height.getMonth() + 1, // JavaScript months are 0-indexed, but we want 1-indexed
            day: height.getDate(),
          },
        };

  return await invoke<SetRestoreHeightArgs, SetRestoreHeightResponse>(
    "set_monero_restore_height",
    args,
  );
}

export async function setMoneroWalletPassword(
  password: string,
): Promise<SetMoneroWalletPasswordResponse> {
  return await invoke<
    SetMoneroWalletPasswordArgs,
    SetMoneroWalletPasswordResponse
  >("set_monero_wallet_password", { password });
}

export async function getMoneroHistory(): Promise<GetMoneroHistoryResponse> {
  return await invokeNoArgs<GetMoneroHistoryResponse>("get_monero_history");
}

export async function getMoneroMainAddress(): Promise<GetMoneroMainAddressResponse> {
  return await invokeNoArgs<GetMoneroMainAddressResponse>(
    "get_monero_main_address",
  );
}

export async function getMoneroSubAddresses(
  accountIndex: number = 0,
): Promise<SubaddressSummary[]> {
  const resp = await invoke<
    GetMoneroSubaddressesArgs,
    GetMoneroSubaddressesResponse
  >("get_monero_subaddresses", {
    account_index: accountIndex,
  });
  return resp.subaddresses;
}

export async function createMoneroSubaddress(
  label: string,
  accountIndex: number = 0,
): Promise<SubaddressSummary> {
  const resp = await invoke<
    CreateMoneroSubaddressArgs,
    CreateMoneroSubaddressResponse
  >("create_monero_subaddress", {
    account_index: accountIndex,
    label,
  });
  return resp.subaddress;
}

export async function setMoneroSubaddressLabel(
  accountIndex: number,
  addressIndex: number,
  label: string,
): Promise<boolean> {
  const resp = await invoke<
    SetMoneroSubaddressLabelArgs,
    SetMoneroSubaddressLabelResponse
  >("set_monero_subaddress_label", {
    account_index: accountIndex,
    address_index: addressIndex,
    label,
  });
  return resp.success;
}

export async function getMoneroBalance(): Promise<GetMoneroBalanceResponse> {
  return await invokeNoArgs<GetMoneroBalanceResponse>("get_monero_balance");
}

export async function sendMonero(
  args: SendMoneroArgs,
): Promise<SendMoneroResponse> {
  return await invoke<SendMoneroArgs, SendMoneroResponse>("send_monero", args);
}

export async function getMoneroSyncProgress(): Promise<GetMoneroSyncProgressResponse> {
  return await invokeNoArgs<GetMoneroSyncProgressResponse>(
    "get_monero_sync_progress",
  );
}

export async function getMoneroSeed(): Promise<GetMoneroSeedResponse> {
  return await invokeNoArgs<GetMoneroSeedResponse>("get_monero_seed");
}

export async function getMoneroSeedAndRestoreHeight(): Promise<
  [GetMoneroSeedResponse, GetRestoreHeightResponse]
> {
  return Promise.all([getMoneroSeed(), getRestoreHeight()]);
}

// Wallet management functions that handle Redux dispatching
export async function initializeMoneroWallet() {
  try {
    await Promise.all([
      getMoneroMainAddress().then((response) => {
        store.dispatch(setMainAddress(response.address));
      }),
      getMoneroBalance().then((response) => {
        store.dispatch(setBalance(response));
      }),
      getMoneroSyncProgress().then((response) => {
        store.dispatch(setSyncProgress(response));
      }),
      getMoneroHistory().then((response) => {
        store.dispatch(setHistory(response));
      }),
      getRestoreHeight().then((response) => {
        store.dispatch(setRestoreHeight(response));
      }),
      getMoneroSubAddresses().then((subaddresses) => {
        store.dispatch(setSubaddresses(subaddresses));
      }),
    ]);
  } catch (err) {
    console.error("Failed to fetch Monero wallet data:", err);
  }
}

export async function sendMoneroTransaction(
  args: SendMoneroArgs,
): Promise<SendMoneroResponse> {
  try {
    const response = await sendMonero(args);

    // Refresh balance and history after sending - but don't let this block the response
    Promise.all([getMoneroBalance(), getMoneroHistory()])
      .then(([newBalance, newHistory]) => {
        store.dispatch(setBalance(newBalance));
        store.dispatch(setHistory(newHistory));
      })
      .catch((refreshErr) => {
        console.error("Failed to refresh wallet data after send:", refreshErr);
      });

    return response;
  } catch (err) {
    console.error("Failed to send Monero:", err);
    throw err;
  }
}

export async function getDataDir(): Promise<string> {
  const testnet = isTestnet();
  return await invoke<GetDataDirArgs, string>("get_data_dir", {
    is_testnet: testnet,
  });
}

export async function resolveApproval<T>(
  requestId: string,
  accept: T,
): Promise<void> {
  try {
    await invoke<ResolveApprovalArgs, ResolveApprovalResponse>(
      "resolve_approval_request",
      { request_id: requestId, accept: accept as object },
    );
  } finally {
    // Always refresh the approval list
    await refreshApprovals();

    // Refresh the approval list a few miliseconds later to again
    // Just to make sure :)
    setTimeout(() => {
      refreshApprovals();
    }, 200);
  }
}

export async function rejectApproval<T>(
  requestId: string,
  reject: T,
): Promise<void> {
  await invoke<RejectApprovalArgs, RejectApprovalResponse>(
    "reject_approval_request",
    { request_id: requestId },
  );
}

export async function refreshApprovals(): Promise<void> {
  const response = await invokeNoArgs<GetPendingApprovalsResponse>(
    "get_pending_approvals",
  );
  store.dispatch(approvalRequestsReplaced(response.approvals));
}

export async function checkSeed(seed: string): Promise<boolean> {
  const response = await invoke<CheckSeedArgs, CheckSeedResponse>(
    "check_seed",
    {
      seed,
    },
  );
  return response.available;
}

export async function saveLogFiles(
  zipFileName: string,
  content: Record<string, string>,
): Promise<void> {
  await invokeUnsafe<void>("save_txt_files", { zipFileName, content });
}

export async function dfxAuthenticate(): Promise<DfxAuthenticateResponse> {
  return await invokeNoArgs<DfxAuthenticateResponse>("dfx_authenticate");
}

export async function changeMoneroNode(
  nodeConfig: MoneroNodeConfig,
): Promise<void> {
  await invoke<{ node_config: MoneroNodeConfig }, void>("change_monero_node", {
    node_config: nodeConfig,
  });
}

export async function refreshP2P(): Promise<void> {
  await invokeNoArgs<void>("refresh_p2p");
}

// Helper function to create MoneroNodeConfig from current settings
export async function getCurrentMoneroNodeConfig(): Promise<MoneroNodeConfig> {
  const network = getNetwork();
  const useMoneroRpcPool = store.getState().settings.useMoneroRpcPool;
  const moneroNodeUrl =
    store.getState().settings.nodes[network][Blockchain.Monero][0] ?? null;

  const moneroNodeConfig =
    useMoneroRpcPool ||
    moneroNodeUrl == null ||
    !(await getMoneroNodeStatus(moneroNodeUrl, network))
      ? { type: "Pool" as const }
      : {
          type: "SingleNode" as const,
          content: {
            url: moneroNodeUrl,
          },
        };

  return moneroNodeConfig;
}

export async function updateMoneroSubaddresses() {
  const subaddresses = await getMoneroSubAddresses();
  store.dispatch(setSubaddresses(subaddresses));
}
