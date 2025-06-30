import { Box, Paper } from "@mui/material";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import SwapStatusAlert from "renderer/components/alert/SwapStatusAlert/SwapStatusAlert";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import { useState } from "react";

export default function SwapWidget() {
  const swap = useAppSelector((state) => state.swap);
  const swapInfo = useActiveSwapInfo();
  
  const [debug, setDebug] = useState(false);

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <SwapStatusAlert swap={swapInfo} onlyShowIfUnusualAmountOfTimeHasPassed />
      <Paper
        elevation={3}
        sx={{
          width: "100%",
          maxWidth: 800,
          margin: "0 auto",
          borderRadius: 2,
          padding: 2,
          display: "flex",
                flexDirection: "column",
                gap: 2,
                justifyContent: "space-between",
                flex: 1,
        }}
      >
        {
          debug ? (
            <DebugPage />
          ) : (
            <>
              <SwapStatePage state={swap.state} />
            </>
          )
        }
        {swap.state !== null && (
          <>
            <SwapStateStepper state={swap.state} />
            <Box sx={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
              <CancelButton />
              <DebugPageSwitchBadge enabled={debug} setEnabled={setDebug} />
            </Box>
          </>
        )}
      </Paper>
    </Box>
  );
}
