import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedMakerStatus, MakerStatus } from "models/apiModel";
import {
  GetSwapInfoResponse,
  ContextStatus,
  TauriTimelockChangeEvent,
  BackgroundRefundState,
  ApprovalRequest,
  TauriBackgroundProgressWrapper,
  TauriBackgroundProgress,
} from "models/tauriModel";
import { MoneroRecoveryResponse } from "../../models/rpcModel";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";
import logger from "utils/logger";

interface State {
  withdrawTxId: string | null;
  rendezvousDiscoveredSellers: (ExtendedMakerStatus | MakerStatus)[];
  swapInfos: {
    [swapId: string]: GetSwapInfoResponseExt;
  };
  moneroRecovery: {
    swapId: string;
    keys: MoneroRecoveryResponse;
  } | null;
  backgroundRefund: {
    swapId: string;
    state: BackgroundRefundState;
  } | null;
  approvalRequests: {
    // Store the full event, keyed by request_id
    [requestId: string]: ApprovalRequest;
  };
  background: {
    [key: string]: TauriBackgroundProgress;
  };
}

export enum ContextStatusType {
  Status = "status",
  Error = "error",
}

export type ResultContextStatus =
  | { type: ContextStatusType.Status; status: ContextStatus }
  | { type: ContextStatusType.Error; error: string };

export interface RPCSlice {
  status: ResultContextStatus | null;
  state: State;
}

const initialState: RPCSlice = {
  status: null,
  state: {
    withdrawTxId: null,
    rendezvousDiscoveredSellers: [],
    swapInfos: {},
    moneroRecovery: null,
    background: {},
    backgroundRefund: null,
    approvalRequests: {},
  },
};

export const rpcSlice = createSlice({
  name: "rpc",
  initialState,
  reducers: {
    contextStatusEventReceived(slice, action: PayloadAction<ContextStatus>) {
      // Don't overwrite error state
      //
      // Once we're in an error state, stay there
      if (slice.status?.type === ContextStatusType.Error) {
        return;
      }

      slice.status = { type: ContextStatusType.Status, status: action.payload };
    },
    contextInitializationFailed(slice, action: PayloadAction<string>) {
      slice.status = { type: ContextStatusType.Error, error: action.payload };
    },
    timelockChangeEventReceived(
      slice: RPCSlice,
      action: PayloadAction<TauriTimelockChangeEvent>,
    ) {
      if (slice.state.swapInfos[action.payload.swap_id]) {
        slice.state.swapInfos[action.payload.swap_id].timelock =
          action.payload.timelock;
      } else {
        logger.warn(
          `Received timelock change event for unknown swap ${action.payload.swap_id}`,
        );
      }
    },
    rpcSetWithdrawTxId(slice, action: PayloadAction<string>) {
      slice.state.withdrawTxId = action.payload;
    },
    rpcSetRendezvousDiscoveredMakers(
      slice,
      action: PayloadAction<(ExtendedMakerStatus | MakerStatus)[]>,
    ) {
      slice.state.rendezvousDiscoveredSellers = action.payload;
    },
    rpcResetWithdrawTxId(slice) {
      slice.state.withdrawTxId = null;
    },
    rpcSetSwapInfo(slice, action: PayloadAction<GetSwapInfoResponse>) {
      slice.state.swapInfos[action.payload.swap_id] =
        action.payload as GetSwapInfoResponseExt;
    },
    rpcSetMoneroRecoveryKeys(
      slice,
      action: PayloadAction<[string, MoneroRecoveryResponse]>,
    ) {
      const swapId = action.payload[0];
      const keys = action.payload[1];

      slice.state.moneroRecovery = {
        swapId,
        keys,
      };
    },
    rpcResetMoneroRecoveryKeys(slice) {
      slice.state.moneroRecovery = null;
    },
    rpcSetBackgroundRefundState(
      slice,
      action: PayloadAction<{ swap_id: string; state: BackgroundRefundState }>,
    ) {
      slice.state.backgroundRefund = {
        swapId: action.payload.swap_id,
        state: action.payload.state,
      };
    },
    approvalEventReceived(slice, action: PayloadAction<ApprovalRequest>) {
      const event = action.payload;
      const requestId = event.request_id;
      slice.state.approvalRequests[requestId] = event;
    },
    approvalRequestsReplaced(slice, action: PayloadAction<ApprovalRequest[]>) {
      // Clear existing approval requests and replace with new ones
      slice.state.approvalRequests = {};
      action.payload.forEach((approval) => {
        slice.state.approvalRequests[approval.request_id] = approval;
      });
    },
    backgroundProgressEventReceived(
      slice,
      action: PayloadAction<TauriBackgroundProgressWrapper>,
    ) {
      slice.state.background[action.payload.id] = action.payload.event;
    },
    backgroundProgressEventRemoved(slice, action: PayloadAction<string>) {
      delete slice.state.background[action.payload];
    },
    rpcSetBackgroundItems(
      slice,
      action: PayloadAction<{ [key: string]: TauriBackgroundProgress }>,
    ) {
      slice.state.background = action.payload;
    },
    rpcSetApprovalItems(
      slice,
      action: PayloadAction<{ [requestId: string]: ApprovalRequest }>,
    ) {
      slice.state.approvalRequests = action.payload;
    },
  },
});

export const {
  contextStatusEventReceived,
  contextInitializationFailed,
  rpcSetWithdrawTxId,
  rpcResetWithdrawTxId,
  rpcSetRendezvousDiscoveredMakers,
  rpcSetSwapInfo,
  rpcSetMoneroRecoveryKeys,
  rpcResetMoneroRecoveryKeys,
  rpcSetBackgroundRefundState,
  timelockChangeEventReceived,
  approvalEventReceived,
  approvalRequestsReplaced,
  backgroundProgressEventReceived,
  backgroundProgressEventRemoved,
  rpcSetBackgroundItems,
  rpcSetApprovalItems,
} = rpcSlice.actions;

export default rpcSlice.reducer;
