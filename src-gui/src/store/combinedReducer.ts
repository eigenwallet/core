import alertsSlice from "./features/alertsSlice";
import makersSlice from "./features/makersSlice";
import ratesSlice from "./features/ratesSlice";
import rpcSlice from "./features/rpcSlice";
import swapReducer from "./features/swapSlice";
import settingsSlice from "./features/settingsSlice";
import nodesSlice from "./features/nodesSlice";
import conversationsSlice from "./features/conversationsSlice";
import poolSlice from "./features/poolSlice";
import walletSlice from "./features/walletSlice";
import logsSlice from "./features/logsSlice";

export const reducers = {
  swap: swapReducer,
  makers: makersSlice,
  rpc: rpcSlice,
  alerts: alertsSlice,
  rates: ratesSlice,
  settings: settingsSlice,
  nodes: nodesSlice,
  conversations: conversationsSlice,
  pool: poolSlice,
  wallet: walletSlice,
  logs: logsSlice,
};
