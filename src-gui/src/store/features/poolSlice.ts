import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { PoolStatus } from "models/tauriModel";

interface PoolSlice {
  status: PoolStatus | null;
  isLoading: boolean;
}

const initialState: PoolSlice = {
  status: null,
  isLoading: true,
};

export const poolSlice = createSlice({
  name: "pool",
  initialState,
  reducers: {
    poolStatusReceived(slice, action: PayloadAction<PoolStatus>) {
      slice.status = action.payload;
      slice.isLoading = false;
    },
  },
});

export const { poolStatusReceived } = poolSlice.actions;

export default poolSlice.reducer;
