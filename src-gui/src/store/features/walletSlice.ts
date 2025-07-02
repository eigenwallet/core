import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import {
  GetMoneroBalanceResponse,
  GetMoneroHistoryResponse,
  GetMoneroMainAddressResponse,
  GetMoneroSyncProgressResponse,
  SendMoneroResponse,
} from "models/tauriModel";

interface WalletState {
  // Wallet data
  mainAddress: string | null;
  balance: GetMoneroBalanceResponse | null;
  syncProgress: GetMoneroSyncProgressResponse | null;
  history: GetMoneroHistoryResponse | null;

  // Loading states
  isLoading: boolean;
  isRefreshing: boolean;
  isSending: boolean;

  // Error states
  error: string | null;

  // Send transaction state
  sendResult: SendMoneroResponse | null;
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
    isLoading: false,
    isRefreshing: false,
    isSending: false,

    // Error states
    error: null,

    // Send transaction state
    sendResult: null,
  },
};

export const walletSlice = createSlice({
  name: "wallet",
  initialState,
  reducers: {
    // Loading state actions
    setLoading(slice, action: PayloadAction<boolean>) {
      slice.state.isLoading = action.payload;
    },
    setRefreshing(slice, action: PayloadAction<boolean>) {
      slice.state.isRefreshing = action.payload;
    },
    setSending(slice, action: PayloadAction<boolean>) {
      slice.state.isSending = action.payload;
    },

    // Error state actions
    setError(slice, action: PayloadAction<string | null>) {
      slice.state.error = action.payload;
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

    // Send result actions
    setSendResult(slice, action: PayloadAction<SendMoneroResponse | null>) {
      slice.state.sendResult = action.payload;
    },

    // Reset actions
    resetWalletState(slice) {
      slice.state = initialState.state;
    },
    clearError(slice) {
      slice.state.error = null;
    },
    clearSendResult(slice) {
      slice.state.sendResult = null;
    },
  },
});

export const {
  setLoading,
  setRefreshing,
  setSending,
  setError,
  setMainAddress,
  setBalance,
  setSyncProgress,
  setHistory,
  setSendResult,
  resetWalletState,
  clearError,
  clearSendResult,
} = walletSlice.actions;

export default walletSlice.reducer;
