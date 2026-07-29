import { useEffect, useEffectEvent, useState } from "react";

/**
  Throttle rerenders to at most once per specified interval or
  if one of the flushOn values changes.
  */
export function useThrottle<T>(value: T, interval_ms: number, flushOn: unknown[] = []) {
  const [cache, setCache] = useState(value);
  const update = useEffectEvent(() => setCache(value));

  useEffect(() => {
    const interval_id = setInterval(update, interval_ms);
    return () => clearInterval(interval_id)
  }, [interval_ms])

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => update(), flushOn)

  return cache;
}

