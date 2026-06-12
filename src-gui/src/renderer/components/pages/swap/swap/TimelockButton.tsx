import { Alert, Button, Dialog, DialogActions } from "@mui/material";
import KeyboardArrowRightIcon from "@mui/icons-material/KeyboardArrowRight";
import { useState } from "react";
import { isGetSwapInfoResponseRunningSwap } from "models/tauriModelExt";
import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import { selectSwapTimelock } from "store/selectors";
import { useAppSelector, useSwapInfo } from "store/hooks";

export default function TimelockButton({ swapId }: { swapId: string }) {
  const [open, setOpen] = useState(false);
  const swap = useSwapInfo(swapId);
  const timelock = useAppSelector(selectSwapTimelock(swapId));

  if (swap == null) return null;
  if (!isGetSwapInfoResponseRunningSwap(swap)) return null;
  if (timelock == null) return null;

  // Only show once the Bitcoin lock has more than three confirmations; before
  // that the "running for a while" hint is premature. In `None`, `blocks_left`
  // counts down from `cancel_timelock`; any other state is past expiry.
  const btcLockConfirmations =
    timelock.type === "None"
      ? swap.cancel_timelock - timelock.content.blocks_left
      : swap.cancel_timelock;
  if (btcLockConfirmations <= 3) return null;

  return (
    <>
      <Alert
        severity="warning"
        variant="filled"
        onClick={() => setOpen(true)}
        icon={false}
        action={<KeyboardArrowRightIcon fontSize="small" />}
        sx={{
          cursor: "pointer",
          // The parent Paper clips us against its rounded top edge.
          borderRadius: 0,
          py: 0.5,
          px: 2,
          alignItems: "center",
          "& .MuiAlert-message": { py: 0 },
          "& .MuiAlert-action": { py: 0, mr: 0 },
        }}
      >
        Swap has taken quite a while...
      </Alert>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        fullWidth
        maxWidth="sm"
        PaperProps={{
          sx: {
            overflow: "hidden",
            bgcolor: "warning.main",
            color: "warning.contrastText",
          },
        }}
      >
        <SwapStatusAlert swap={swap} />
        <DialogActions>
          <Button
            variant="outlined"
            color="inherit"
            onClick={() => setOpen(false)}
          >
            Close
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
