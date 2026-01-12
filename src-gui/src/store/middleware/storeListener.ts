import { createListenerMiddleware } from "@reduxjs/toolkit";
import { throttle, debounce } from "lodash";
import {
  getAllSwapInfos,
  getAllSwapTimelocks,
  checkBitcoinBalance,
  getBitcoinAddress,
  updateAllNodeStatuses,
  getSwapInfo,
  getSwapTimelock,
  initializeMoneroWallet,
  changeMoneroNode,
  getCurrentMoneroNodeConfig,
} from "renderer/rpc";
import logger from "utils/logger";
import {
  contextStatusEventReceived,
  ContextStatusType,
} from "store/features/rpcSlice";
import {
  addNode,
  setFetchFiatPrices,
  setFiatCurrency,
  setUseMoneroRpcPool,
} from "store/features/settingsSlice";
import { Blockchain, Network } from "store/types";
import { fetchFeedbackMessagesViaHttp, updateRates } from "renderer/api";
import { RootState, store } from "renderer/store/storeRenderer";
import { swapProgressEventReceived } from "store/features/swapSlice";
import {
  addFeedbackId,
  setConversation,
} from "store/features/conversationsSlice";
import { setBitcoinAddress } from "store/features/bitcoinWalletSlice";

// Create a Map to store throttled functions per swap_id
const throttledGetSwapInfoFunctions = new Map<
  string,
  ReturnType<typeof throttle>
>();

// Function to get or create a throttled getSwapInfo for a specific swap_id
const getThrottledSwapInfoUpdater = (swapId: string) => {
  if (!throttledGetSwapInfoFunctions.has(swapId)) {
    // Create a throttled function that executes at most once every 2 seconds
    // but will wait for 3 seconds of quiet during rapid calls (using debounce)
    const debouncedGetSwapInfo = debounce(() => {
      logger.debug(`Executing getSwapInfo for swap ${swapId}`);
      getSwapInfo(swapId).catch((error) => {
        logger.debug(`Failed to fetch swap info for swap ${swapId}: ${error}`);
      });
      getSwapTimelock(swapId).catch((error) => {
        logger.debug(`Failed to fetch timelock for swap ${swapId}: ${error}`);
      });
    }, 3000); // 3 seconds debounce for rapid calls

    const throttledFunction = throttle(debouncedGetSwapInfo, 2000, {
      leading: true, // Execute immediately on first call
      trailing: true, // Execute on trailing edge if needed
    });

    throttledGetSwapInfoFunctions.set(swapId, throttledFunction);
  }

  return throttledGetSwapInfoFunctions.get(swapId)!;
};

export function createMainListeners() {
  const listener = createListenerMiddleware();

  // Listener for when the Context status state changes
  // When the context becomes available, we check the bitcoin balance, fetch all swap infos and connect to the rendezvous point
  listener.startListening({
    predicate: (action, currentState, previousState) => {
      const currentStatus = (currentState as RootState).rpc.status;
      const previousStatus = (previousState as RootState).rpc.status;

      // Only trigger if the status actually changed
      return currentStatus !== previousStatus;
    },
    effect: async (action, api) => {
      const currentStatus = (api.getState() as RootState).rpc.status;
      const previousStatus = (api.getOriginalState() as RootState).rpc.status;

      const status =
        currentStatus?.type === ContextStatusType.Status
          ? currentStatus.status
          : null;
      const previousContextStatus =
        previousStatus?.type === ContextStatusType.Status
          ? previousStatus.status
          : null;

      if (!status) return;

      const initTasks: Promise<void>[] = [];

      const bitcoinWalletJustBecameAvailable =
        status.bitcoin_wallet_available &&
        !previousContextStatus?.bitcoin_wallet_available;
      const moneroWalletJustBecameAvailable =
        status.monero_wallet_available &&
        !previousContextStatus?.monero_wallet_available;
      const databaseJustBecameAvailable =
        status.database_available && !previousContextStatus?.database_available;

      // If the Bitcoin wallet just came available, check the Bitcoin balance and get address
      if (bitcoinWalletJustBecameAvailable) {
        initTasks.push(
          (async () => {
            logger.info(
              "Bitcoin wallet just became available, checking balance and getting address...",
            );
            await Promise.all([
              checkBitcoinBalance(),
              getBitcoinAddress()
                .then((address) => store.dispatch(setBitcoinAddress(address)))
                .catch((error) =>
                  logger.error("Failed to fetch Bitcoin address", error),
                ),
            ]);
          })(),
        );
      }

      // If the Monero wallet just came available, initialize the Monero wallet
      if (moneroWalletJustBecameAvailable) {
        initTasks.push(
          (async () => {
            logger.info("Monero wallet just became available, initializing...");
            await initializeMoneroWallet();

            // Also set the Monero node to the current one
            const nodeConfig = await getCurrentMoneroNodeConfig();
            await changeMoneroNode(nodeConfig);
          })(),
        );
      }

      // If the database or the Bitcoin wallet just came available fetch swaps
      if (databaseJustBecameAvailable || bitcoinWalletJustBecameAvailable) {
        initTasks.push(
          (async () => {
            logger.info(
              "Database or Bitcoin wallet just became available, fetching swaps...",
            );
            await Promise.all([getAllSwapInfos(), getAllSwapTimelocks()]);
          })(),
        );
      }

      await Promise.all(initTasks);
    },
  });

  // Listener for:
  // - when a swap is released (fetch bitcoin balance)
  // - when a swap progress event is received (update the swap info)
  listener.startListening({
    actionCreator: swapProgressEventReceived,
    effect: async (action) => {
      // Skip Tauri calls when mocking is enabled (DEV only)
      if (store.getState().swap._mockOnlyDisableTauriCallsOnSwapProgress) {
        return;
      }

      if (action.payload.event.type === "Released") {
        logger.info("Swap released, updating bitcoin balance...");
        await checkBitcoinBalance();
      }

      // Update the swap info using throttled function
      logger.info(
        "Swap progress event received, scheduling throttled swap info update...",
      );
      const throttledUpdater = getThrottledSwapInfoUpdater(
        action.payload.swap_id,
      );
      throttledUpdater();
    },
  });

  // Update the rates when the fiat currency is changed
  listener.startListening({
    actionCreator: setFiatCurrency,
    effect: async () => {
      if (store.getState().settings.fetchFiatPrices) {
        logger.info("Fiat currency changed, updating rates...");
        await updateRates();
      }
    },
  });

  // Update the rates when fetching fiat prices is enabled
  listener.startListening({
    actionCreator: setFetchFiatPrices,
    effect: async (action) => {
      if (action.payload === true) {
        logger.info("Activated fetching fiat prices, updating rates...");
        await updateRates();
      }
    },
  });

  // Update the node status when a new one is added
  listener.startListening({
    actionCreator: addNode,
    effect: async (_) => {
      await updateAllNodeStatuses();
    },
  });

  // Listener for Monero node configuration changes
  listener.startListening({
    actionCreator: setUseMoneroRpcPool,
    effect: async (action) => {
      const usePool = action.payload;
      logger.info(
        `Monero node setting changed to: ${usePool ? "Pool" : "Single Node"}`,
      );

      try {
        const nodeConfig = await getCurrentMoneroNodeConfig();
        await changeMoneroNode(nodeConfig);
        logger.info({ nodeConfig }, "Changed Monero node configuration to: ");
      } catch (error) {
        logger.error({ error }, "Failed to change Monero node configuration:");
      }
    },
  });

  // Listener for when a feedback id is added
  listener.startListening({
    actionCreator: addFeedbackId,
    effect: async (action) => {
      // Whenever a new feedback id is added, fetch the messages and store them in the Redux store
      const messages = await fetchFeedbackMessagesViaHttp(action.payload);
      store.dispatch(setConversation({ feedbackId: action.payload, messages }));
    },
  });

  return listener;
}
