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

      // If a swap is *terminally* released while still in the offer phase
      // (e.g. the user cancelled before any funds were committed) there is no
      // meaningful final state worth keeping around — drop it. A Released
      // event carrying `next_auto_resume_at_unix_ms` is just a retry signal,
      // not a terminal release, so we keep the entry around so the UI can
      // show the retry banner.
      if (
        action.payload.event.type === "Released" &&
        action.payload.event.content.next_auto_resume_at_unix_ms == null &&
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
        // Preserve `prev` as the last *non-Released* event. Two consecutive
        // Released events (e.g. `make_swap` fails before `bob::run` emits
        // any progress, so we go straight from one retry-Released to the
        // next) would otherwise squash the meaningful prior state and force
        // every consumer to walk further back themselves.
        if (existingSwap.curr.type !== "Released") {
          existingSwap.prev = existingSwap.curr;
        }
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
