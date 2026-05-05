import { Link } from "@mui/material";
import { SwapState } from "models/storeModel";
import { haveFundsBeenLocked } from "models/tauriModelExt";
import { suspendSwap } from "renderer/rpc";
import { useState } from "react";
import SwapSuspendAlert from "renderer/components/modal/SwapSuspendAlert";
import { useAppDispatch } from "store/hooks";
import { swapProgressRemoved } from "store/features/swapSlice";

export default function CancelButton({ swapState }: { swapState: SwapState }) {
  const [openSuspendAlert, setOpenSuspendAlert] = useState(false);
  const dispatch = useAppDispatch();

  const isReleased = swapState.curr.type === "Released";
  const hasFundsBeenLocked = haveFundsBeenLocked(swapState.curr);

  async function suspend() {
    await suspendSwap(swapState.swapId);
  }

  async function onCancel() {
    if (isReleased) {
      // Swap is already done; "Close" just dismisses the final-state panel.
      dispatch(swapProgressRemoved(swapState.swapId));
      return;
    }

    if (hasFundsBeenLocked) {
      setOpenSuspendAlert(true);
      return;
    }

    await suspend();
  }

  const label = isReleased
    ? "Close"
    : hasFundsBeenLocked
      ? "Suspend"
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
