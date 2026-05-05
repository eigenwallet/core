import { Link } from "@mui/material";
import { SwapState } from "models/storeModel";
import { haveFundsBeenLocked } from "models/tauriModelExt";
import { suspendSwap } from "renderer/rpc";
import { useState } from "react";
import SwapSuspendAlert from "renderer/components/modal/SwapSuspendAlert";

export default function CancelButton({ swapState }: { swapState: SwapState }) {
  const [openSuspendAlert, setOpenSuspendAlert] = useState(false);

  const hasFundsBeenLocked = haveFundsBeenLocked(swapState.curr);

  async function suspend() {
    await suspendSwap(swapState.swapId);
  }

  async function onCancel() {
    if (hasFundsBeenLocked) {
      setOpenSuspendAlert(true);
      return;
    }

    await suspend();
  }

  const label =
    hasFundsBeenLocked && swapState.curr.type !== "Released"
      ? "Suspend"
      : swapState.curr.type === "Released"
        ? "Close"
        : "Cancel";

  return (
    <>
      <SwapSuspendAlert
        open={openSuspendAlert}
        onClose={() => setOpenSuspendAlert(false)}
        onSuspend={suspend}
      />
      <Link
        component="button"
        type="button"
        onClick={onCancel}
        variant="caption"
        color="text.secondary"
        underline="hover"
      >
        {label}
      </Link>
    </>
  );
}
