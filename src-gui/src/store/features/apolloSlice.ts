import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ApolloStatus } from "models/tauriModel";

interface ApolloSlice {
  status: ApolloStatus | null;
}

const initialState: ApolloSlice = {
  status: null,
};

export const apolloSlice = createSlice({
  name: "apollo",
  initialState,
  reducers: {
    apolloStatusReceived(slice, action: PayloadAction<ApolloStatus>) {
      slice.status = action.payload;
    },
  },
});

export const { apolloStatusReceived } = apolloSlice.actions;

export default apolloSlice.reducer;
