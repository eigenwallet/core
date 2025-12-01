import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import {
  PeerQuoteProgress,
  ConnectionChange,
  ConnectionStatus,
  QuoteStatus,
} from "models/tauriModel";
import { exhaustiveGuard } from "utils/typescriptUtils";

interface P2PSlice {
  connectionStatus: Record<string, ConnectionStatus>;
  lastAddress: Record<string, string>;
  quoteStatus: Record<string, QuoteStatus>;
}

const initialState: P2PSlice = {
  connectionStatus: {},
  lastAddress: {},
  quoteStatus: {},
};

export const p2pSlice = createSlice({
  name: "p2p",
  initialState,
  reducers: {
    quotesProgressReceived(slice, action: PayloadAction<PeerQuoteProgress[]>) {
      action.payload.forEach((entry) => {
        slice.quoteStatus[entry.peer_id] = entry.quote_status;
      });
    },
    connectionChangeReceived(
      slice,
      action: PayloadAction<{ peer_id: string; change: ConnectionChange }>,
    ) {
      const { peer_id, change } = action.payload;

      switch (change.type) {
        case "Connection":
          slice.connectionStatus[peer_id] = change.content;
          break;
        case "LastAddress":
          slice.lastAddress[peer_id] = change.content;
          break;
        default:
          exhaustiveGuard(change);
      }
    },
  },
});

export const { quotesProgressReceived, connectionChangeReceived } =
  p2pSlice.actions;

export default p2pSlice.reducer;
