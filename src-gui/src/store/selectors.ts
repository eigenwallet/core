import { createSelector } from "@reduxjs/toolkit";
import { RootState } from "./storeRenderer";

const selectRpcState = (state: RootState) => state.rpc.state;

export const selectAllSwapIds = createSelector(
  [selectRpcState],
  (rpcState) => Object.keys(rpcState.swapInfos)
);

export const selectAllSwapInfos = createSelector(
  [selectRpcState],
  (rpcState) => Object.values(rpcState.swapInfos)
);

export const selectPendingApprovals = createSelector(
  [selectRpcState],
  (rpcState) => Object.values(rpcState.approvalRequests).filter(
    (c) => c.request_status.state === "Pending"
  )
);
