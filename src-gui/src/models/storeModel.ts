import { CliLog, SwapSpawnType } from "./cliModel";
import { TauriSwapProgressEvent } from "./tauriModel";

export type SwapState = {
  curr: TauriSwapProgressEvent;
  prev: TauriSwapProgressEvent | null;
  swapId: string;
};

export interface SwapSlice {
  swaps: Record<string, SwapState>;
  logs: CliLog[];
  spawnType: SwapSpawnType | null;
  /** DEV ONLY: When true, prevents Tauri calls in the swap progress listener */
  _mockOnlyDisableTauriCallsOnSwapProgress: boolean;
}
