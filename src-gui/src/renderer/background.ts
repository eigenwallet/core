import { listen } from "@tauri-apps/api/event";
import { TauriEvent } from "models/tauriModel";
import {
  contextStatusEventReceived,
  contextInitializationFailed,
  timelockChangeEventReceived,
  approvalEventReceived,
  backgroundProgressEventReceived,
} from "store/features/rpcSlice";
import { setBitcoinBalance } from "store/features/bitcoinWalletSlice";
import { receivedCliLog } from "store/features/logsSlice";
import { poolStatusReceived } from "store/features/poolSlice";
import { swapProgressEventReceived } from "store/features/swapSlice";
import {
  quotesProgressReceived,
  connectionChangeReceived,
} from "store/features/p2pSlice";
import logger from "utils/logger";
import { fetchAllConversations, updateAlerts, updateRates } from "./api";
import {
  checkContextStatus,
  getSwapInfo,
  getSwapTimelock,
  initializeContext,
  refreshApprovals,
  updateAllNodeStatuses,
} from "./rpc";
import { store } from "./store/storeRenderer";
import { exhaustiveGuard } from "utils/typescriptUtils";
import {
  setBalance,
  setHistory,
  setSyncProgress,
} from "store/features/walletSlice";
import {
  applyDefaultNodes,
  validateDonateToDevelopmentTip,
} from "store/features/settingsSlice";
import {
  DEFAULT_NODES,
  NEGATIVE_NODES_MAINNET,
  NEGATIVE_NODES_TESTNET,
} from "store/defaults";

const TAURI_UNIFIED_EVENT_CHANNEL_NAME = "tauri-unified-event";

// Update node statuses every 2 minutes
const STATUS_UPDATE_INTERVAL = 2 * 60 * 1_000;

// Update the exchange rate every 1 minute
const UPDATE_RATE_INTERVAL = 1 * 60 * 1_000;

// Fetch all conversations every 10 minutes
const FETCH_CONVERSATIONS_INTERVAL = 10 * 60 * 1_000;

// Fetch pending approvals every 2 seconds
const FETCH_PENDING_APPROVALS_INTERVAL = 2 * 1_000;

// Check context status every 2 seconds
const CHECK_CONTEXT_STATUS_INTERVAL = 2 * 1_000;

function setIntervalImmediate(callback: () => void, interval: number): void {
  callback();
  setInterval(callback, interval);
}

export async function setupBackgroundTasks(): Promise<void> {
  // Apply default nodes on startup (removes broken nodes, adds new ones)
  store.dispatch(
    applyDefaultNodes({
      defaultNodes: DEFAULT_NODES,
      negativeNodesMainnet: NEGATIVE_NODES_MAINNET,
      negativeNodesTestnet: NEGATIVE_NODES_TESTNET,
    }),
  );

  // Validate donation tip setting
  store.dispatch(validateDonateToDevelopmentTip());

  // Setup periodic fetch tasks
  setIntervalImmediate(updateAllNodeStatuses, STATUS_UPDATE_INTERVAL);
  setIntervalImmediate(updateRates, UPDATE_RATE_INTERVAL);
  setIntervalImmediate(fetchAllConversations, FETCH_CONVERSATIONS_INTERVAL);
  setIntervalImmediate(refreshApprovals, FETCH_PENDING_APPROVALS_INTERVAL);

  // Fetch all alerts
  updateAlerts();

  // Setup Tauri event listeners
  // Check if the context is already available. This is to prevent unnecessary re-initialization
  setIntervalImmediate(async () => {
    const contextStatus = await checkContextStatus();
    store.dispatch(contextStatusEventReceived(contextStatus));
  }, CHECK_CONTEXT_STATUS_INTERVAL);

  const contextStatus = await checkContextStatus();

  // If all components are unavailable, we need to initialize the context
  if (
    !contextStatus.bitcoin_wallet_available &&
    !contextStatus.monero_wallet_available &&
    !contextStatus.database_available &&
    !contextStatus.tor_available
  )
    // Warning: If we reload the page while the Context is being initialized, this function will throw an error
    initializeContext().catch((e) => {
      logger.error(
        e,
        "Failed to initialize context on page load. This might be because we reloaded the page while the context was being initialized",
      );
      store.dispatch(contextInitializationFailed(String(e)));
    });
}

// Listen for the unified event
listen<TauriEvent>(TAURI_UNIFIED_EVENT_CHANNEL_NAME, (event) => {
  const { channelName, event: eventData } = event.payload;

  switch (channelName) {
    case "SwapProgress":
      // Skip when mocking is enabled (DEV only) - mock dispatches bypass this listener
      if (!store.getState().swap._mockOnlyDisableTauriCallsOnSwapProgress) {
        store.dispatch(swapProgressEventReceived(eventData));
      }
      break;

    case "CliLog":
      store.dispatch(receivedCliLog(eventData));
      break;

    case "BalanceChange":
      store.dispatch(setBitcoinBalance(eventData.balance));
      break;

    case "SwapDatabaseStateUpdate":
      getSwapInfo(eventData.swap_id).catch((error) => {
        logger.debug(
          `Failed to fetch swap info for swap ${eventData.swap_id}: ${error}`,
        );
      });
      getSwapTimelock(eventData.swap_id).catch((error) => {
        logger.debug(
          `Failed to fetch timelock for swap ${eventData.swap_id}: ${error}`,
        );
      });

      // This is ugly but it's the best we can do for now
      // Sometimes we are too quick to fetch the swap info and the new state is not yet reflected
      // in the database. So we wait a bit before fetching the new state
      setTimeout(() => {
        getSwapInfo(eventData.swap_id).catch((error) => {
          logger.debug(
            `Failed to fetch swap info for swap ${eventData.swap_id}: ${error}`,
          );
        });
        getSwapTimelock(eventData.swap_id).catch((error) => {
          logger.debug(
            `Failed to fetch timelock for swap ${eventData.swap_id}: ${error}`,
          );
        });
      }, 3000);
      break;

    case "TimelockChange":
      store.dispatch(timelockChangeEventReceived(eventData));
      break;

    case "Approval":
      store.dispatch(approvalEventReceived(eventData));
      break;

    case "BackgroundProgress":
      store.dispatch(backgroundProgressEventReceived(eventData));
      break;

    case "PoolStatusUpdate":
      store.dispatch(poolStatusReceived(eventData));
      break;

    case "MoneroWalletUpdate":
      console.log("MoneroWalletUpdate", eventData);
      if (eventData.type === "BalanceChange") {
        store.dispatch(setBalance(eventData.content));
      }
      if (eventData.type === "HistoryUpdate") {
        store.dispatch(setHistory(eventData.content));
      }
      if (eventData.type === "SyncProgress") {
        store.dispatch(setSyncProgress(eventData.content));
      }
      break;

    case "P2P":
      if (eventData.type === "ConnectionChange") {
        store.dispatch(connectionChangeReceived(eventData.content));
      }
      if (eventData.type === "QuotesProgress") {
        store.dispatch(quotesProgressReceived(eventData.content.peers));
      }
      break;

    default:
      exhaustiveGuard(channelName);
  }
});
