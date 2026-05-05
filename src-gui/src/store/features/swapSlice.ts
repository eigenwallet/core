import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { TauriSwapProgressEventWrapper } from "models/tauriModel";
import { isOfferPhase } from "models/tauriModelExt";
import { SwapSlice } from "../../models/storeModel";

const initialState: SwapSlice = {
  swaps: {},
  logs: [],

  // TODO: Remove this and replace logic entirely with Tauri events
  spawnType: null,

  _mockOnlyDisableTauriCallsOnSwapProgress: false,
};

export const swapSlice = createSlice({
  name: "swap",
  initialState,
  reducers: {
    swapProgressEventReceived(
      swap,
      action: PayloadAction<TauriSwapProgressEventWrapper>,
    ) {
      const existingSwap = swap.swaps[action.payload.swap_id];

      // If a swap is released while still in the offer phase (e.g. the user
      // cancelled before any funds were committed) there is no meaningful
      // final state worth keeping around — drop it instead of leaving a
      // panel for the user to acknowledge.
      if (
        action.payload.event.type === "Released" &&
        existingSwap != null &&
        isOfferPhase(existingSwap.curr)
      ) {
        delete swap.swaps[action.payload.swap_id];
        return;
      }

      if (existingSwap == null) {
        swap.swaps[action.payload.swap_id] = {
          curr: action.payload.event,
          prev: null,
          swapId: action.payload.swap_id,
        };
      } else {
        existingSwap.prev = existingSwap.curr;
        existingSwap.curr = action.payload.event;
      }
    },
    swapReset() {
      return initialState;
    },
    swapProgressRemoved(swap, action: PayloadAction<string>) {
      delete swap.swaps[action.payload];
    },
    setMockOnlyDisableTauriCallsOnSwapProgress(
      swap,
      action: PayloadAction<boolean>,
    ) {
      swap._mockOnlyDisableTauriCallsOnSwapProgress = action.payload;
    },
  },
});

export const {
  swapReset,
  swapProgressEventReceived,
  swapProgressRemoved,
  setMockOnlyDisableTauriCallsOnSwapProgress,
} = swapSlice.actions;

export default swapSlice.reducer;
