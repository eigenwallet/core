import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TransactionInfo } from "models/tauriModel";

interface BitcoinWalletState {
  address: string | null;
  balance: number | null;
  history: TransactionInfo[] | null;
}

const initialState: BitcoinWalletState = {
  address: null,
  balance: null,
  history: null,
};

export const bitcoinWalletSlice = createSlice({
  name: "bitcoinWallet",
  initialState,
  reducers: {
    setBitcoinAddress(state, action: PayloadAction<string>) {
      state.address = action.payload;
    },
    setBitcoinBalance(state, action: PayloadAction<number>) {
      state.balance = action.payload;
    },
    setBitcoinHistory(state, action: PayloadAction<TransactionInfo[]>) {
      state.history = action.payload;
    },
    resetBitcoinWalletState(state) {
      return initialState;
    },
  },
});

export const {
  setBitcoinAddress,
  setBitcoinBalance,
  setBitcoinHistory,
  resetBitcoinWalletState,
} = bitcoinWalletSlice.actions;

export default bitcoinWalletSlice.reducer;
