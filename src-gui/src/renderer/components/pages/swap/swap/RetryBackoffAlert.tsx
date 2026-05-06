import { Alert } from "@mui/material";
import KeyboardArrowRightIcon from "@mui/icons-material/KeyboardArrowRight";
import { useEffect, useState } from "react";
import { resumeSwap } from "renderer/rpc";
import { useAppSelector } from "store/hooks";

// Renders only when the swap is currently waiting in the auto-retry backoff
// (`curr.type === "Released"` with `next_auto_resume_at_unix_ms` set). Shows
// the remaining time until the manager will auto-retry; clicking the alert
// pre-empts the wait and resumes immediately.
export default function RetryBackoffAlert({ swapId }: { swapId: string }) {
  const nextRetryAtMs = useAppSelector((state) => {
    const s = state.swap.swaps[swapId];
    if (s == null || s.curr.type !== "Released") return null;
    return s.curr.content.next_auto_resume_at_unix_ms ?? null;
  });

  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (nextRetryAtMs == null) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [nextRetryAtMs]);

  if (nextRetryAtMs == null) return null;

  const secondsLeft = Math.max(0, Math.ceil((nextRetryAtMs - now) / 1000));

  return (
    <Alert
      severity="warning"
      variant="filled"
      onClick={() => {
        void resumeSwap(swapId);
      }}
      action={<KeyboardArrowRightIcon fontSize="small" />}
      sx={{
        cursor: "pointer",
        py: 0.5,
        px: 2,
        alignItems: "center",
        userSelect: "none",
        transition:
          "transform 80ms ease-out, filter 120ms ease-out, box-shadow 120ms ease-out",
        "&:hover": { filter: "brightness(1.05)" },
        "&:active": {
          transform: "scale(0.985)",
          filter: "brightness(0.92)",
          boxShadow: "inset 0 2px 4px rgba(0,0,0,0.25)",
        },
        "& .MuiAlert-message": { py: 0 },
        "& .MuiAlert-action": { py: 0, mr: 0 },
      }}
    >
      Swap encountered an error. Retrying in {secondsLeft}s. Click to resume
      now.
    </Alert>
  );
}
