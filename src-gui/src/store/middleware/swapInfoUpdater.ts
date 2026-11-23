import { throttle } from "lodash";

export const SWAP_INFO_UPDATE_INTERVAL_MS = 2_000;

export const createSwapInfoUpdater = (update: () => void) =>
  throttle(update, SWAP_INFO_UPDATE_INTERVAL_MS, {
    leading: true,
    trailing: true,
  });
