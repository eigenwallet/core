import { sortBy, sum, throttle } from "lodash";
import {
  BobStateName,
  GetSwapInfoResponseExt,
  isBitcoinSyncProgress,
  isPendingBackgroundProcess,
  isPendingLockBitcoinApprovalEvent,
  isPendingSeedSelectionApprovalEvent,
  PendingApprovalRequest,
  PendingLockBitcoinApprovalRequest,
  PendingSelectMakerApprovalRequest,
  isPendingSelectMakerApprovalEvent,
  haveFundsBeenLocked,
  PendingSeedSelectionApprovalRequest,
  PendingSendMoneroApprovalRequest,
  isPendingSendMoneroApprovalEvent,
  PendingPasswordApprovalRequest,
  isPendingPasswordApprovalEvent,
  isContextFullyInitialized,
} from "models/tauriModelExt";
import { TypedUseSelectorHook, useDispatch, useSelector } from "react-redux";
import type { AppDispatch, RootState } from "renderer/store/storeRenderer";
import { parseDateString } from "utils/parseUtils";
import { useEffect, useMemo, useState } from "react";
import { isCliLogRelatedToSwap } from "models/cliModel";
import { SettingsState } from "./features/settingsSlice";
import { NodesSlice } from "./features/nodesSlice";
import { RatesState } from "./features/ratesSlice";
import {
  TauriBackgroundProgress,
  TauriBitcoinSyncProgress,
} from "models/tauriModel";
import { Alert } from "models/apiModel";
import { fnv1a } from "utils/hash";
import {
  selectAllSwapInfos,
  selectPendingApprovals,
  selectSwapInfoWithTimelock,
  selectSwapInfosRaw,
} from "./selectors";

export const useAppDispatch = () => useDispatch<AppDispatch>();
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;

export function useResumeableSwapsCount(
  additionalFilter?: (s: GetSwapInfoResponseExt) => boolean,
) {
  const saneSwapInfos = useSaneSwapInfos();

  return useAppSelector(
    (state) =>
      saneSwapInfos.filter(
        (swapInfo: GetSwapInfoResponseExt) =>
          !swapInfo.completed &&
          (additionalFilter == null || additionalFilter(swapInfo)),
      ).length,
  );
}

/**
 * Counts the number of resumeable swaps excluding:
 * - Punished swaps
 * - Swaps where the sanity check was not passed (e.g. they were aborted)
 */
export function useResumeableSwapsCountExcludingPunished() {
  return useResumeableSwapsCount(
    (s) =>
      s.state_name !== BobStateName.BtcPunished &&
      s.state_name !== BobStateName.SwapSetupCompleted,
  );
}

/// Returns true if we have any swap that is running
export function useIsSwapRunning() {
  return useAppSelector(
    (state) =>
      state.swap.state !== null && state.swap.state.curr.type !== "Released",
  );
}

/// Returns true if we have a swap that is running and
/// that swap has any funds locked
export function useIsSwapRunningAndHasFundsLocked() {
  const swapInfo = useActiveSwapInfo();
  const swapTauriState = useAppSelector(
    (state) => state.swap.state?.curr ?? null,
  );

  // If the swap is in the Released state, we return false
  if (swapTauriState?.type === "Released") {
    return false;
  }

  // If the tauri state tells us that funds have been locked, we return true
  if (haveFundsBeenLocked(swapTauriState)) {
    return true;
  }

  // If we have a database entry (swapInfo) for this swap, we return true
  if (swapInfo != null) {
    return true;
  }

  return false;
}

/// Returns true if we have a swap that is running
export function useIsSpecificSwapRunning(swapId: string | null) {
  return useAppSelector(
    (state) =>
      swapId != null &&
      state.swap.state !== null &&
      state.swap.state.swapId === swapId &&
      state.swap.state.curr.type !== "Released",
  );
}

export function useIsContextAvailable() {
  return useAppSelector((state) => isContextFullyInitialized(state.rpc.status));
}

/// We do not use a sanity check here, as opposed to the other useSwapInfo hooks,
/// because we are explicitly asking for a specific swap
export function useSwapInfo(
  swapId: string | null,
): GetSwapInfoResponseExt | null {
  return useAppSelector((state) =>
    swapId ? (state.rpc.state.swapInfos?.[swapId] ?? null) : null,
  );
}

export function useActiveSwapId(): string | null {
  return useAppSelector((s) => s.swap.state?.swapId ?? null);
}

export function useActiveSwapInfo(): GetSwapInfoResponseExt | null {
  const swapId = useActiveSwapId();
  return useSwapInfo(swapId);
}

export function useActiveSwapLogs() {
  const swapId = useActiveSwapId();
  const logs = useAppSelector((s) => s.logs.state.logs);

  return useMemo(() => {
    if (swapId == null) {
      return [];
    }

    return logs.filter((log) => isCliLogRelatedToSwap(log.log, swapId));
  }, [logs, swapId]);
}

/// This hook returns the all swap infos, as an array
/// Excluding those who are in a state where it's better to hide them from the user
export function useSaneSwapInfos() {
  const swapInfos = useAppSelector(selectAllSwapInfos);
  return swapInfos.filter((swap) => {
    // We hide swaps that are in the SwapSetupCompleted state
    // This is because they are probably ones where:
    // 1. The user force stopped the swap while we were waiting for their confirmation of the offer
    // 2. We where therefore unable to transition to SafelyAborted
    if (swap.state_name === BobStateName.SwapSetupCompleted) {
      return false;
    }

    // We hide swaps that were safely aborted
    // No funds were locked. Cannot be resumed.
    // Wouldn't be beneficial to show them to the user
    if (swap.state_name === BobStateName.SafelyAborted) {
      return false;
    }

    return true;
  });
}

/// This hook returns the swap infos sorted by date
export function useSwapInfosSortedByDate() {
  const swapInfos = useSaneSwapInfos();

  return sortBy(swapInfos, (swap) => -parseDateString(swap.start_date));
}

/// Returns true if swapInfos has been loaded
/// False means means we haven't fetched the swap infos yet
export function useAreSwapInfosLoaded(): boolean {
  const swapInfos = useAppSelector(selectSwapInfosRaw);
  return swapInfos !== null;
}

export function useSettings<T>(selector: (settings: SettingsState) => T): T {
  const settings = useAppSelector((state) => state.settings);
  return selector(settings);
}

export function useNodes<T>(selector: (nodes: NodesSlice) => T): T {
  const nodes = useAppSelector((state) => state.nodes);
  return selector(nodes);
}

export function usePendingApprovals(): PendingApprovalRequest[] {
  return useAppSelector(selectPendingApprovals) as PendingApprovalRequest[];
}

export function usePendingLockBitcoinApproval(): PendingLockBitcoinApprovalRequest[] {
  const approvals = usePendingApprovals();
  return approvals.filter((c) => isPendingLockBitcoinApprovalEvent(c));
}

export function usePendingSendMoneroApproval(): PendingSendMoneroApprovalRequest[] {
  const approvals = usePendingApprovals();
  return approvals.filter((c) => isPendingSendMoneroApprovalEvent(c));
}

export function usePendingSelectMakerApproval(): PendingSelectMakerApprovalRequest[] {
  const approvals = usePendingApprovals();
  return approvals.filter((c) => isPendingSelectMakerApprovalEvent(c));
}

export function usePendingSeedSelectionApproval(): PendingSeedSelectionApprovalRequest[] {
  const approvals = usePendingApprovals();
  return approvals.filter((c) => isPendingSeedSelectionApprovalEvent(c));
}

export function usePendingPasswordApproval(): PendingPasswordApprovalRequest[] {
  const approvals = usePendingApprovals();
  return approvals.filter((c) => isPendingPasswordApprovalEvent(c));
}

/// Returns all the pending background processes
/// In the format [id, {componentName, {type: "Pending", content: {consumed, total}}}]
export function usePendingBackgroundProcesses(): [
  string,
  TauriBackgroundProgress,
][] {
  const background = useAppSelector((state) => state.rpc.state.background);
  return Object.entries(background).filter(([_, c]) =>
    isPendingBackgroundProcess(c),
  );
}

export function useBitcoinSyncProgress(): TauriBitcoinSyncProgress[] {
  const pendingProcesses = usePendingBackgroundProcesses();
  const syncingProcesses = pendingProcesses
    .map(([_, c]) => c)
    .filter(isBitcoinSyncProgress);
  return syncingProcesses
    .map((c) => c.progress.content)
    .filter(
      (content): content is TauriBitcoinSyncProgress => content !== undefined,
    );
}

export function useIsSyncingBitcoin(): boolean {
  const syncProgress = useBitcoinSyncProgress();
  return syncProgress.length > 0;
}

/// This function returns the cumulative sync progress of all currently running Bitcoin wallet syncs
/// If all syncs are unknown, it returns {type: "Unknown"}
/// If at least one sync is known, it returns {type: "Known", content: {consumed, total}}
/// where consumed and total are the sum of all the consumed and total values of the syncs
export function useConservativeBitcoinSyncProgress(): TauriBitcoinSyncProgress | null {
  const syncingProcesses = useBitcoinSyncProgress();
  const progressValues = syncingProcesses.map((c) => c.content?.consumed ?? 0);
  const totalValues = syncingProcesses.map((c) => c.content?.total ?? 0);

  const progress = sum(progressValues);
  const total = sum(totalValues);

  // If either the progress or the total is 0, we consider the sync to be unknown
  if (progress === 0 || total === 0) {
    return {
      type: "Unknown",
    };
  }

  return {
    type: "Known",
    content: {
      consumed: progress,
      total: total,
    },
  };
}

/**
 * Calculates the number of unread messages from staff for a specific feedback conversation.
 * @param feedbackId The ID of the feedback conversation.
 * @returns The number of unread staff messages.
 */
export function useUnreadMessagesCount(feedbackId: string): number {
  const { conversationsMap, seenMessagesSet } = useAppSelector((state) => ({
    conversationsMap: state.conversations.conversations,
    // Convert seenMessages array to a Set for efficient lookup
    seenMessagesSet: new Set(state.conversations.seenMessages),
  }));

  const messages = conversationsMap[feedbackId] || [];

  const unreadStaffMessages = messages.filter(
    (msg) => msg.is_from_staff && !seenMessagesSet.has(msg.id.toString()),
  );

  return unreadStaffMessages.length;
}

/**
 * Calculates the total number of unread messages from staff across all feedback conversations.
 * @returns The total number of unread staff messages.
 */
export function useTotalUnreadMessagesCount(): number {
  const { conversationsMap, seenMessagesSet } = useAppSelector((state) => ({
    conversationsMap: state.conversations.conversations,
    seenMessagesSet: new Set(state.conversations.seenMessages),
  }));

  let totalUnreadCount = 0;
  for (const feedbackId in conversationsMap) {
    const messages = conversationsMap[feedbackId] || [];
    const unreadStaffMessages = messages.filter(
      (msg) => msg.is_from_staff && !seenMessagesSet.has(msg.id.toString()),
    );
    totalUnreadCount += unreadStaffMessages.length;
  }

  return totalUnreadCount;
}

/// Returns all the alerts that have not been acknowledged
export function useAlerts(): Alert[] {
  return useAppSelector((state) =>
    state.alerts.alerts.filter(
      (alert) =>
        // Check if there is an acknowledgement with
        // the same id and the same title hash
        !state.alerts.acknowledgedAlerts.some(
          (ack) => ack.id === alert.id && ack.titleHash === fnv1a(alert.title),
        ),
    ),
  );
}

/// Returns true if the we heuristically determine that the user has used the app at least a little bit
/// We don't want to annoy completely new users with a bunch of stuff
export function useIsExperiencedUser(): boolean {
  // Returns true if either:
  // - the Monero wallet balance > 0
  // - the Bitcoin wallet balance > 0
  // - the user has made at least 1 swap
  const moneroBalance = useAppSelector(
    (state) => state.wallet.state.balance?.total_balance,
  );
  const bitcoinBalance = useAppSelector((state) => state.bitcoinWallet.balance);
  const swapCount = useAppSelector(selectAllSwapInfos).length;

  const hasMoneroBalance =
    moneroBalance !== undefined && parseFloat(moneroBalance) > 0;
  const hasBitcoinBalance =
    bitcoinBalance !== null &&
    bitcoinBalance !== undefined &&
    bitcoinBalance > 0;
  const hasSwaps = swapCount > 0;

  return hasMoneroBalance || hasBitcoinBalance || hasSwaps;
}

/**
 * Hook that returns true if the user has been idle (no mouse/keyboard activity) for a given duration.
 * Uses throttling to avoid excessive event handler calls during rapid input.
 */
export function useIsIdle(idleTimeMs: number): boolean {
  const [isIdle, setIsIdle] = useState(false);

  useEffect(() => {
    let timeoutId: number;

    const handleTimeout = () => {
      setIsIdle(true);
    };

    const handleEvent = throttle(() => {
      setIsIdle(false);
      window.clearTimeout(timeoutId);
      timeoutId = window.setTimeout(handleTimeout, idleTimeMs);
    }, 500);

    const handleVisibilityChange = () => {
      if (!document.hidden) {
        handleEvent();
      }
    };

    timeoutId = window.setTimeout(handleTimeout, idleTimeMs);

    window.addEventListener("mousemove", handleEvent);
    window.addEventListener("mousedown", handleEvent);
    window.addEventListener("resize", handleEvent);
    window.addEventListener("keydown", handleEvent);
    window.addEventListener("touchstart", handleEvent);
    window.addEventListener("wheel", handleEvent);
    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      window.removeEventListener("mousemove", handleEvent);
      window.removeEventListener("mousedown", handleEvent);
      window.removeEventListener("resize", handleEvent);
      window.removeEventListener("keydown", handleEvent);
      window.removeEventListener("touchstart", handleEvent);
      window.removeEventListener("wheel", handleEvent);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.clearTimeout(timeoutId);
      handleEvent.cancel();
    };
  }, [idleTimeMs]);

  return isIdle;
}
