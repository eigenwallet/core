import { Box, Button, Dialog, DialogActions, Paper } from "@mui/material";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import { useState, useCallback } from "react";

const swapWidgetContainerSx = {
  display: "flex",
  flexDirection: "column",
  gap: 2,
  width: "100%",
};

const paperSx = {
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
};

const swapStateContainerSx = {
  display: "flex",
  minHeight: "30vh",
  flexDirection: "column",
  justifyContent: "center",
};

const actionsContainerSx = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
};

export default function SwapWidget() {
  const swap = useAppSelector((state) => state.swap);
  const swapInfo = useActiveSwapInfo();

  const [debug, setDebug] = useState(false);

  const handleCloseDebug = useCallback(() => {
    setDebug(false);
  }, []);

  return (
    <Box sx={swapWidgetContainerSx}>
      {swapInfo != null && (
        <SwapStatusAlert
          swap={swapInfo}
          onlyShowIfUnusualAmountOfTimeHasPassed
        />
      )}
      <Dialog
        fullWidth
        maxWidth="md"
        open={debug}
        onClose={handleCloseDebug}
      >
        <DebugPage />
        <DialogActions>
          <Button variant="outlined" onClick={handleCloseDebug}>
            Close
          </Button>
        </DialogActions>
      </Dialog>
      <Paper elevation={3} sx={paperSx}>
        <Box sx={swapStateContainerSx}>
          <SwapStatePage state={swap.state} />
        </Box>
        {swap.state !== null && (
          <>
            <SwapStateStepper state={swap.state} />
            <Box sx={actionsContainerSx}>
              <CancelButton />
              <DebugPageSwitchBadge enabled={debug} setEnabled={setDebug} />
            </Box>
          </>
        )}
      </Paper>
    </Box>
  );
}
