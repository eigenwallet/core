import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriLogEvent } from "models/tauriModel";
import { parseLogsFromString } from "utils/parseUtils";
import { CliLog } from "models/cliModel";
import { fnv1a } from "utils/hash";

export type HashedLog = {
  log: CliLog | string;
  hash: string;
};

interface LogsState {
  logs: HashedLog[];
}

export interface LogsSlice {
  state: LogsState;
}

const initialState: LogsSlice = {
  state: {
    logs: [],
  },
};

function serializeLog(log: CliLog | string): string {
  if (typeof log === "string") {
    return `str:${log}`;
  }

  const parts = [
    "obj",
    log.timestamp,
    log.level,
    log.target ?? "",
    JSON.stringify(log.fields),
  ];

  if (log.spans != null && log.spans.length > 0) {
    parts.push(JSON.stringify(log.spans));
  }

  return parts.join("|");
}

function createHashedLog(log: CliLog | string): HashedLog {
  return {
    log,
    hash: fnv1a(serializeLog(log)),
  };
}

export function hashLogs(logs: (CliLog | string)[]): HashedLog[] {
  return logs.map(createHashedLog);
}

export const logsSlice = createSlice({
  name: "logs",
  initialState,
  reducers: {
    receivedCliLog(slice, action: PayloadAction<TauriLogEvent>) {
      const parsedLogs = parseLogsFromString(action.payload.buffer);
      const hashedLogs = parsedLogs.map(createHashedLog);
      for (const entry of hashedLogs) {
        slice.state.logs.push(entry);
      }
    },
    clearLogs(slice) {
      slice.state.logs = [];
    },
  },
});

export const { receivedCliLog, clearLogs } = logsSlice.actions;

export default logsSlice.reducer;
