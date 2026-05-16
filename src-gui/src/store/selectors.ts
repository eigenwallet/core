import { createSelector } from "@reduxjs/toolkit";
import { RootState } from "renderer/store/storeRenderer";
import {
  GetSwapInfoResponseExt,
  isBitcoinSyncProgress,
  isPendingBackgroundProcess,
  isPendingLockBitcoinApprovalEvent,
  isPendingPasswordApprovalEvent,
  isPendingSeedSelectionApprovalEvent,
  isPendingSelectMakerApprovalEvent,
  isPendingSendMoneroApprovalEvent,
} from "models/tauriModelExt";
import {
  ConnectionStatus,
  ExpiredTimelocks,
  QuoteStatus,
  TauriBitcoinSyncProgress,
} from "models/tauriModel";

const selectRpcState = (state: RootState) => state.rpc.state;
const selectP2pState = (state: RootState) => state.p2p;

export const selectSwapInfosRaw = createSelector(
  [selectRpcState],
  (rpcState) => rpcState.swapInfos,
);

export const selectAllSwapIds = createSelector([selectRpcState], (rpcState) =>
  rpcState.swapInfos ? Object.keys(rpcState.swapInfos) : [],
);

export const selectAllSwapInfos = createSelector(
  [selectRpcState],
  (rpcState) => (rpcState.swapInfos ? Object.values(rpcState.swapInfos) : []),
);

export const selectSwapTimelocks = createSelector(
  [selectRpcState],
  (rpcState) => rpcState.swapTimelocks,
);

export const selectSwapTimelock = (swapId: string | null) =>
  createSelector([selectSwapTimelocks], (timelocks) =>
    swapId ? (timelocks[swapId] ?? null) : null,
  );

export const selectSwapInfoWithTimelock = (swapId: string) =>
  createSelector(
    [selectRpcState],
    (
      rpcState,
    ):
      | (GetSwapInfoResponseExt & { timelock: ExpiredTimelocks | null })
      | null => {
      if (!rpcState.swapInfos) return null;
      const swapInfo = rpcState.swapInfos[swapId];
      if (!swapInfo) return null;
      return {
        ...swapInfo,
        timelock: rpcState.swapTimelocks[swapId] ?? null,
      };
    },
  );

export const selectPendingApprovals = createSelector(
  [selectRpcState],
  (rpcState) =>
    Object.values(rpcState.approvalRequests).filter(
      (c) => c.request_status.state === "Pending",
    ),
);

export const selectPendingLockBitcoinApprovals = createSelector(
  [selectPendingApprovals],
  (approvals) => approvals.filter(isPendingLockBitcoinApprovalEvent),
);

export const selectPendingSendMoneroApprovals = createSelector(
  [selectPendingApprovals],
  (approvals) => approvals.filter(isPendingSendMoneroApprovalEvent),
);

export const selectPendingSelectMakerApprovals = createSelector(
  [selectPendingApprovals],
  (approvals) => approvals.filter(isPendingSelectMakerApprovalEvent),
);

export const selectPendingSeedSelectionApprovals = createSelector(
  [selectPendingApprovals],
  (approvals) => approvals.filter(isPendingSeedSelectionApprovalEvent),
);

export const selectPendingPasswordApprovals = createSelector(
  [selectPendingApprovals],
  (approvals) => approvals.filter(isPendingPasswordApprovalEvent),
);

export const selectPendingBackgroundProcesses = createSelector(
  [selectRpcState],
  (rpcState) =>
    Object.entries(rpcState.background).filter(([, progress]) =>
      isPendingBackgroundProcess(progress),
    ),
);

export const selectBitcoinSyncProgress = createSelector(
  [selectPendingBackgroundProcesses],
  (pendingProcesses): TauriBitcoinSyncProgress[] =>
    pendingProcesses
      .map(([, progress]) => progress)
      .filter(isBitcoinSyncProgress)
      .map((progress) => progress.progress.content)
      .filter(
        (content): content is TauriBitcoinSyncProgress =>
          content !== undefined,
      ),
);

export const selectConservativeBitcoinSyncProgress = createSelector(
  [selectBitcoinSyncProgress],
  (syncingProcesses): TauriBitcoinSyncProgress | null => {
    const progress = syncingProcesses.reduce(
      (total, current) => total + (current.content?.consumed ?? 0),
      0,
    );
    const total = syncingProcesses.reduce(
      (sum, current) => sum + (current.content?.total ?? 0),
      0,
    );

    if (progress === 0 || total === 0) {
      return {
        type: "Unknown",
      };
    }

    return {
      type: "Known",
      content: {
        consumed: progress,
        total,
      },
    };
  },
);

// TODO: This should be split into multiple selectors/hooks to avoid excessive re-rendering
export const selectPeers = createSelector([selectP2pState], (p2p) => {
  const peerIds = new Set([
    ...Object.keys(p2p.connectionStatus),
    ...Object.keys(p2p.lastAddress),
    ...Object.keys(p2p.quoteStatus),
  ]);

  return Array.from(peerIds).map((peerId) => ({
    peer_id: peerId,
    connection: p2p.connectionStatus[peerId] ?? null,
    last_address: p2p.lastAddress[peerId] ?? null,
    quote: p2p.quoteStatus[peerId] ?? null,
  }));
});
