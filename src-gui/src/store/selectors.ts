import { createSelector } from "@reduxjs/toolkit";
import { RootState } from "renderer/store/storeRenderer";
import { BobStateName, GetSwapInfoResponseExt } from "models/tauriModelExt";
import {
  ConnectionStatus,
  ExpiredTimelocks,
  QuoteStatus,
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

export type SwapReputation = {
  successfulSwaps: number;
  refundedSwaps: number;
  failedSwaps: number;
};

export const EMPTY_SWAP_REPUTATION: SwapReputation = {
  successfulSwaps: 0,
  refundedSwaps: 0,
  failedSwaps: 0,
};

const REFUNDED_SWAP_STATES = new Set<BobStateName>([
  BobStateName.BtcRefunded,
  BobStateName.BtcEarlyRefunded,
  BobStateName.BtcMercyConfirmed,
]);

const FAILED_SWAP_STATES = new Set<BobStateName>([
  BobStateName.BtcPunished,
  BobStateName.BtcWithheld,
]);

export const selectSwapReputationByPeer = createSelector(
  [selectSwapInfosRaw],
  (swapInfos) => {
    const reputationByPeer: Record<string, SwapReputation> = {};

    if (!swapInfos) return reputationByPeer;

    for (const swap of Object.values(swapInfos)) {
      const peerId = swap.seller.peer_id;
      const reputation =
        reputationByPeer[peerId] ??
        (reputationByPeer[peerId] = {
          successfulSwaps: 0,
          refundedSwaps: 0,
          failedSwaps: 0,
        });

      if (swap.state_name === BobStateName.XmrRedeemed) {
        reputation.successfulSwaps += 1;
      } else if (REFUNDED_SWAP_STATES.has(swap.state_name)) {
        reputation.refundedSwaps += 1;
      } else if (FAILED_SWAP_STATES.has(swap.state_name)) {
        reputation.failedSwaps += 1;
      }
    }

    return reputationByPeer;
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
