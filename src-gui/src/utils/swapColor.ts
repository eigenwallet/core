import { fnv1a } from "./hash";

/** Derives a stable color for a swap id so users can recognize it across views. */
export function swapIdColor(swapId: string, alpha = 1): string {
  const hue = parseInt(fnv1a(swapId), 16) % 360;
  return `hsla(${hue}, 45%, 68%, ${alpha})`;
}
