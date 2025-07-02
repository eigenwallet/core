import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import {
  GetMoneroBalanceResponse,
  GetMoneroHistoryResponse,
  GetMoneroSyncProgressResponse,
} from "models/tauriModel";

interface WalletState {
  // Wallet data
  mainAddress: string | null;
  balance: GetMoneroBalanceResponse | null;
  syncProgress: GetMoneroSyncProgressResponse | null;
  history: GetMoneroHistoryResponse | null;

  // Loading states
  isRefreshing: boolean;
  isSending: boolean;
}

export interface WalletSlice {
  state: WalletState;
}

const initialState: WalletSlice = {
  state: {
    // Wallet data
    mainAddress: null,
    balance: null,
    syncProgress: null,
    history: null,

    // Loading states
    isRefreshing: false,
    isSending: false,
  },
};

export const walletSlice = createSlice({
  name: "wallet",
  initialState,
  reducers: {
    setRefreshing(slice, action: PayloadAction<boolean>) {
      slice.state.isRefreshing = action.payload;
    },
    setSending(slice, action: PayloadAction<boolean>) {
      slice.state.isSending = action.payload;
    },

    // Wallet data actions
    setMainAddress(slice, action: PayloadAction<string>) {
      slice.state.mainAddress = action.payload;
    },
    setBalance(slice, action: PayloadAction<GetMoneroBalanceResponse>) {
      slice.state.balance = action.payload;
    },
    setSyncProgress(
      slice,
      action: PayloadAction<GetMoneroSyncProgressResponse>,
    ) {
      slice.state.syncProgress = action.payload;
    },
    setHistory(slice, action: PayloadAction<GetMoneroHistoryResponse>) {
      slice.state.history = action.payload;
    },
    // Reset actions
    resetWalletState(slice) {
      slice.state = initialState.state;
    },
  },
});

export const {
  setRefreshing,
  setSending,
  setMainAddress,
  setBalance,
  setSyncProgress,
  setHistory,
  resetWalletState,
} = walletSlice.actions;

export default walletSlice.reducer;
