import { fnv1a } from "./hash";

/**
 * Derives a stable, visually distinct color for a given swap id. Used to
 * give each swap a subtle visual identity (underline + top border) so users
 * can recognize the same swap across different views.
 */
export function swapIdColor(swapId: string, alpha = 1): string {
  const hue = parseInt(fnv1a(swapId), 16) % 360;
  return `hsla(${hue}, 45%, 68%, ${alpha})`;
}
