import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriLogEvent } from "models/tauriModel";
import { parseLogsFromString } from "utils/parseUtils";
import { CliLog } from "models/cliModel";
import { fnv1a } from "utils/hash";

/// We only keep the last 5000 logs in the store
const MAX_LOG_ENTRIES = 5000;

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

export type HashedLog = {
  log: CliLog | string;
  hash: string;
};

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

      // If we have too many logs, discard 1/10 of them (oldest logs)
      // We explictly discard more than we need to, such that we don't have to
      // do this too often
      if (slice.state.logs.length > MAX_LOG_ENTRIES) {
        const removeCount = Math.floor(slice.state.logs.length / 10);
        slice.state.logs = slice.state.logs.slice(removeCount);
      }
    },
  },
});

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

export const { receivedCliLog } = logsSlice.actions;

export default logsSlice.reducer;
