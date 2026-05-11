import { createSelector } from "@reduxjs/toolkit";
import { RootState } from "renderer/store/storeRenderer";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";
import {
  ConnectionStatus,
  ExpiredTimelocks,
  QuoteStatus,
} from "models/tauriModel";

const selectRpcState = (state: RootState) => state.rpc.state;
const selectP2pState = (state: RootState) => state.p2p;
const selectConnectionStatus = (state: RootState) => state.p2p.connectionStatus;
const selectQuoteStatus = (state: RootState) => state.p2p.quoteStatus;

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

export const selectConnectedPeerIds = createSelector(
  [selectConnectionStatus],
  (connectionStatus) =>
    Object.entries(connectionStatus)
      .filter(([, status]) => status === ConnectionStatus.Connected)
      .map(([peerId]) => peerId),
);

export const selectMakerDiscoveryActivity = createSelector(
  [selectConnectionStatus, selectQuoteStatus],
  (connectionStatus, quoteStatus) => ({
    dialsInflight: Object.values(connectionStatus).filter(
      (status) => status === ConnectionStatus.Dialing,
    ).length,
    quotesInflight: Object.values(quoteStatus).filter(
      (status) => status === QuoteStatus.Inflight,
    ).length,
  }),
);

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
