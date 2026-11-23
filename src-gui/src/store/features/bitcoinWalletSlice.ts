import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { BalanceResponse } from "models/tauriModel";

interface BitcoinWalletState {
  address: string | null;
  balance: number | null;
  // The part of `balance` that withdrawals and swaps can spend
  spendableBalance: number | null;
}

const initialState: BitcoinWalletState = {
  address: null,
  balance: null,
  spendableBalance: null,
};

export const bitcoinWalletSlice = createSlice({
  name: "bitcoinWallet",
  initialState,
  reducers: {
    setBitcoinAddress(state, action: PayloadAction<string>) {
      state.address = action.payload;
    },
    setBitcoinBalance(state, action: PayloadAction<BalanceResponse>) {
      state.balance = action.payload.balance;
      state.spendableBalance = action.payload.spendable;
    },
  },
});

export const { setBitcoinAddress, setBitcoinBalance } =
  bitcoinWalletSlice.actions;

export default bitcoinWalletSlice.reducer;
