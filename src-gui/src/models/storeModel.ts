import { CliLog, SwapSpawnType } from "./cliModel";
import { BidQuote, TauriSwapProgressEvent } from "./tauriModel";

export type SwapState = {
  curr: TauriSwapProgressEvent;
  prev: TauriSwapProgressEvent | null;
  swapId: string;
  /// The quote of the maker that the user selected for this swap.
  /// It is null until the GUI receives the `ReceivedQuote` event.
  quote: BidQuote | null;
};

export interface SwapSlice {
  state: SwapState | null;
  logs: CliLog[];
  spawnType: SwapSpawnType | null;
  /** DEV ONLY: When true, prevents Tauri calls in the swap progress listener */
  _mockOnlyDisableTauriCallsOnSwapProgress: boolean;
}
