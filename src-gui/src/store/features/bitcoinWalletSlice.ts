import { createSlice, PayloadAction } from "@reduxjs/toolkit";

interface BitcoinWalletState {
  address: string | null;
  balance: number | null;
}

const initialState: BitcoinWalletState = {
  address: null,
  balance: null,
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
    resetBitcoinWalletState(state) {
      return initialState;
    },
  },
});

export const { setBitcoinAddress, setBitcoinBalance, resetBitcoinWalletState } =
  bitcoinWalletSlice.actions;

export default bitcoinWalletSlice.reducer;
