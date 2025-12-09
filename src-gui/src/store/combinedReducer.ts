import alertsSlice from "./features/alertsSlice";
import ratesSlice from "./features/ratesSlice";
import rpcSlice from "./features/rpcSlice";
import swapReducer from "./features/swapSlice";
import settingsSlice from "./features/settingsSlice";
import nodesSlice from "./features/nodesSlice";
import conversationsSlice from "./features/conversationsSlice";
import poolSlice from "./features/poolSlice";
import walletSlice from "./features/walletSlice";
import bitcoinWalletSlice from "./features/bitcoinWalletSlice";
import logsSlice from "./features/logsSlice";
import p2pSlice from "./features/p2pSlice";

export const reducers = {
  swap: swapReducer,
  rpc: rpcSlice,
  p2p: p2pSlice,
  alerts: alertsSlice,
  rates: ratesSlice,
  settings: settingsSlice,
  nodes: nodesSlice,
  conversations: conversationsSlice,
  pool: poolSlice,
  wallet: walletSlice,
  bitcoinWallet: bitcoinWalletSlice,
  logs: logsSlice,
};
