import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createSwapInfoUpdater,
  SWAP_INFO_UPDATE_INTERVAL_MS,
} from "./swapInfoUpdater";

afterEach(() => {
  vi.useRealTimers();
});

describe("createSwapInfoUpdater", () => {
  it("updates immediately on the first progress event", () => {
    vi.useFakeTimers();
    const update = vi.fn();
    const updater = createSwapInfoUpdater(update);

    updater();

    expect(update).toHaveBeenCalledOnce();
    updater.cancel();
  });

  it("coalesces rapid progress events into one trailing update", () => {
    vi.useFakeTimers();
    const update = vi.fn();
    const updater = createSwapInfoUpdater(update);

    updater();
    updater();
    updater();

    expect(update).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(SWAP_INFO_UPDATE_INTERVAL_MS - 1);
    expect(update).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(1);
    expect(update).toHaveBeenCalledTimes(2);
    updater.cancel();
  });

  it("keeps updating during a continuous stream of progress events", () => {
    vi.useFakeTimers();
    const update = vi.fn();
    const updater = createSwapInfoUpdater(update);

    updater();
    for (
      let elapsed = 0;
      elapsed < SWAP_INFO_UPDATE_INTERVAL_MS * 2;
      elapsed += 500
    ) {
      vi.advanceTimersByTime(500);
      updater();
    }

    expect(update).toHaveBeenCalledTimes(3);
    updater.cancel();
  });
});
