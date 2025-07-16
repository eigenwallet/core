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
  isLoading: boolean;
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
    isLoading: true,
  },
};

export const walletSlice = createSlice({
  name: "wallet",
  initialState,
  reducers: {
    setRefreshing(slice, action: PayloadAction<boolean>) {
      slice.state.isRefreshing = action.payload;
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
    setIsLoading(slice, action: PayloadAction<boolean>) {
      slice.state.isLoading = action.payload;
    },
    // Reset actions
    resetWalletState(slice) {
      slice.state = initialState.state;
    },
  },
});

export const {
  setRefreshing,
  setMainAddress,
  setBalance,
  setSyncProgress,
  setHistory,
  resetWalletState,
  setIsLoading,
} = walletSlice.actions;

export default walletSlice.reducer;
