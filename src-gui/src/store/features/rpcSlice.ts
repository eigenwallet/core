import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import {
  GetSwapInfoResponse,
  ContextStatus,
  TauriTimelockChangeEvent,
  ApprovalRequest,
  TauriBackgroundProgressWrapper,
  TauriBackgroundProgress,
  ExpiredTimelocks,
} from "models/tauriModel";
import { MoneroRecoveryResponse } from "../../models/rpcModel";
import { GetSwapInfoResponseExt } from "models/tauriModelExt";

interface State {
  swapInfos: {
    [swapId: string]: GetSwapInfoResponseExt;
  } | null;
  swapTimelocks: {
    [swapId: string]: ExpiredTimelocks;
  };
  moneroRecovery: {
    swapId: string;
    keys: MoneroRecoveryResponse;
  } | null;
  approvalRequests: {
    // Store the full event, keyed by request_id
    [requestId: string]: ApprovalRequest;
  };
  background: {
    [key: string]: TauriBackgroundProgress;
  };
  torForcedExcuse: string;
  updaterProxy: string | null;
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
    swapInfos: null,
    swapTimelocks: {},
    moneroRecovery: null,
    background: {},
    approvalRequests: {},
    torForcedExcuse: "",
    updaterProxy: null,
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
      if (action.payload.timelock) {
        slice.state.swapTimelocks[action.payload.swap_id] =
          action.payload.timelock;
      }
    },
    rpcSetSwapInfo(slice, action: PayloadAction<GetSwapInfoResponse>) {
      if (slice.state.swapInfos === null) {
        slice.state.swapInfos = {};
      }
      slice.state.swapInfos[action.payload.swap_id] =
        action.payload as GetSwapInfoResponseExt;
    },
    rpcSetSwapInfosLoaded(slice) {
      if (slice.state.swapInfos === null) {
        slice.state.swapInfos = {};
      }
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
    rpcSetTorNetworkConfig(
      slice,
      action: PayloadAction<[string, string | null]>,
    ) {
      [slice.state.torForcedExcuse, slice.state.updaterProxy] = action.payload;
    },
  },
});

export const {
  contextStatusEventReceived,
  contextInitializationFailed,
  rpcSetSwapInfo,
  rpcSetSwapInfosLoaded,
  rpcSetMoneroRecoveryKeys,
  rpcResetMoneroRecoveryKeys,
  timelockChangeEventReceived,
  approvalEventReceived,
  approvalRequestsReplaced,
  backgroundProgressEventReceived,
  rpcSetTorNetworkConfig,
} = rpcSlice.actions;

export default rpcSlice.reducer;
