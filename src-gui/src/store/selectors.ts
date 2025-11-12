import { createSelector } from "@reduxjs/toolkit";
import { RootState } from "renderer/store/storeRenderer";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";
import { ExpiredTimelocks } from "models/tauriModel";

const selectRpcState = (state: RootState) => state.rpc.state;

export const selectAllSwapIds = createSelector([selectRpcState], (rpcState) =>
  Object.keys(rpcState.swapInfos),
);

export const selectAllSwapInfos = createSelector([selectRpcState], (rpcState) =>
  Object.values(rpcState.swapInfos),
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
