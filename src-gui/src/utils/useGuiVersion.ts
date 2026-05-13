import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

let cached: string | undefined;
let pending: Promise<string | undefined> | undefined;

/**
 * Returns the GUI's bundled version (from `tauri.conf.json`).
 * The underlying Tauri IPC call runs at most once per app lifetime; all
 * subsequent consumers read the cached value synchronously.
 */
export function useGuiVersion(): string | undefined {
  const [version, setVersion] = useState<string | undefined>(cached);

  useEffect(() => {
    if (cached !== undefined) return;
    pending ??= getVersion().catch((err) => {
      console.warn("Failed to read GUI version from Tauri", err);
      return undefined;
    });
    let cancelled = false;
    pending.then((v) => {
      if (v === undefined) return;
      cached = v;
      if (!cancelled) setVersion(v);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return version;
}
