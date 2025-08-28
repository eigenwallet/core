import {
  Box,
  Button,
  Dialog,
  DialogActions,
  Paper,
  Skeleton,
} from "@mui/material";
import {
  useActiveSwapInfo,
  useAppSelector,
  usePendingSelectOfferApproval,
} from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import { useEffect, useState } from "react";
import { buyXmr } from "renderer/rpc";

export default function SwapWidget() {
  const swap = useAppSelector((state) => state.swap);
  const swapInfo = useActiveSwapInfo();
  const pendingSelectOfferApprovals = usePendingSelectOfferApproval();

  const [debug, setDebug] = useState(false);

  const isWaitingForBtcDeposit =
    swap.state?.curr.type === "WaitingForBtcDeposit";
  const isShowingAddressInput = pendingSelectOfferApprovals.length > 0;

  useEffect(() => {
    if (swap.state === null) {
      buyXmr();
    }
  }, [swap.state]);

  return (
    <Box
      sx={{ display: "flex", flexDirection: "column", gap: 2, width: "100%" }}
    >
      <SwapStatusAlert swap={swapInfo} onlyShowIfUnusualAmountOfTimeHasPassed />
      <Dialog
        fullWidth
        maxWidth="md"
        open={debug}
        onClose={() => setDebug(false)}
      >
        <DebugPage />
        <DialogActions>
          <Button variant="outlined" onClick={() => setDebug(false)}>
            Close
          </Button>
        </DialogActions>
      </Dialog>
      {swap.state === null ? (
        <Skeleton variant="rounded" width="100%" height={400} />
      ) : (
        <Paper
          elevation={3}
          sx={{
            width: "100%",
            maxWidth: 800,
            borderRadius: 2,
            margin: "0 auto",
            padding: 2,
            display: "flex",
            flexDirection: "column",
            gap: 2,
            justifyContent: "space-between",
            flex: 1,
          }}
        >
          <SwapStatePage state={swap.state} />
          {swap.state !== null && (
            <>
              <SwapStateStepper state={swap.state} />
              <Box
                sx={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 2,
                }}
              >
                <Box sx={{ display: "flex", alignItems: "center", gap: 2 }}>
                  {!isWaitingForBtcDeposit && !isShowingAddressInput && (
                    <CancelButton />
                  )}
                  <Box
                    sx={{
                      opacity: 0.6,
                      "&:hover": { opacity: 1 },
                    }}
                  >
                    <DebugPageSwitchBadge
                      enabled={debug}
                      setEnabled={setDebug}
                    />
                  </Box>
                </Box>
              </Box>
            </>
          )}
        </Paper>
      )}
    </Box>
  );
}
