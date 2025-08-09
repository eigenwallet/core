import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriLogEvent, TauriLogIndexEvent } from "models/tauriModel";
import { parseLogsFromString } from "utils/parseUtils";
import { CliLog } from "models/cliModel";

interface LogsState {
  logs: [number, CliLog | string][];
  baseIndex: number | null;
  endIndex: number | null;
  inflight_log_fetching: [number, number][];
}

export interface LogsSlice {
  state: LogsState;
}

const initialState: LogsSlice = {
  state: {
    logs: [],
    baseIndex: null,
    endIndex: null,
    inflight_log_fetching: [],
  },
};

export const logsSlice = createSlice({
  name: "logs",
  initialState,
  reducers: {
    logsWindowReplaced(
      slice,
      action: PayloadAction<[number, CliLog | string][]>,
    ) {
      slice.state.logs = action.payload;
    },
    logsWindowMerged(
      slice,
      action: PayloadAction<[number, CliLog | string][]>,
    ) {
      const existing = new Set(slice.state.logs.map(([i]) => i));
      const merged = slice.state.logs.concat(
        action.payload.filter(([i]) => !existing.has(i)),
      );
      // Keep ordered by index
      merged.sort((a, b) => a[0] - b[0]);
      slice.state.logs = merged;
    },
    receivedCliLogIndex(slice, action: PayloadAction<TauriLogIndexEvent>) {
      slice.state.baseIndex = action.payload.base_index;
      slice.state.endIndex = action.payload.end_index;
    },
    logFetchStarted(slice, action: PayloadAction<[number, number]>) {
      slice.state.inflight_log_fetching.push(action.payload);
    },
    logFetchFinished(slice, action: PayloadAction<[number, number]>) {
      const [start, end] = action.payload;
      slice.state.inflight_log_fetching =
        slice.state.inflight_log_fetching.filter(
          ([s, e]) => s !== start || e !== end,
        );
    },
    requestLogsWindow(
      _slice,
      _action: PayloadAction<{ start: number; end: number }>,
    ) {},
  },
});

export const {
  receivedCliLogIndex,
  logsWindowReplaced,
  logsWindowMerged,
  logFetchStarted,
  logFetchFinished,
  requestLogsWindow,
} = logsSlice.actions;

export default logsSlice.reducer;
