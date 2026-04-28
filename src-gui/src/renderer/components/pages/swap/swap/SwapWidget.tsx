import {
  Box,
  Button,
  Dialog,
  DialogActions,
  Paper,
  Tooltip,
  Typography,
} from "@mui/material";
import { useState } from "react";
import { SwapState } from "models/storeModel";
import { useAppSelector } from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import MockSwapControls from "renderer/components/modal/swap/pages/MockSwapControls";

export default function SwapWidget() {
  const runningSwaps = useAppSelector((state) =>
    Object.values(state.swap.swaps).filter(
      (swap) => swap.curr.type !== "Released",
    ),
  );
  const visibleSwaps = runningSwaps.length > 0 ? runningSwaps : [null];

  return (
    <Box
      sx={{ display: "flex", flexDirection: "column", gap: 2, width: "100%" }}
    >
      {import.meta.env.DEV && <MockSwapControls />}
      <Box
        sx={{
          width: "100%",
          maxWidth: 800,
          margin: "0 auto",
          display: "flex",
          flexDirection: "column",
          gap: 2,
        }}
      >
        {visibleSwaps.map((swap, index) => (
          <SwapStatePanel
            key={swap?.swapId ?? "new-swap"}
            swap={swap}
            index={index}
          />
        ))}
      </Box>
    </Box>
  );
}

function SwapStatePanel({
  swap,
  index,
}: {
  swap: SwapState | null;
  index: number;
}) {
  const [debug, setDebug] = useState(false);

  return (
    <Paper
      elevation={3}
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 2,
        borderRadius: 2,
        padding: 2,
      }}
    >
      {swap != null && (
        <>
          <Box sx={{ display: "flex", flexDirection: "column", gap: 0.25 }}>
            <Typography variant="subtitle2">Swap {index + 1}</Typography>
            <Tooltip title={swap.swapId}>
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {swap.swapId}
              </Typography>
            </Tooltip>
          </Box>
          <Dialog
            fullWidth
            maxWidth="md"
            open={debug}
            onClose={() => setDebug(false)}
          >
            <DebugPage swapId={swap.swapId} />
            <DialogActions>
              <Button variant="outlined" onClick={() => setDebug(false)}>
                Close
              </Button>
            </DialogActions>
          </Dialog>
        </>
      )}
      <Box
        sx={{
          display: "flex",
          minHeight: "30vh",
          flexDirection: "column",
          justifyContent: "center",
        }}
      >
        <SwapStatePage state={swap} />
      </Box>
      {swap != null && <SwapStateStepper state={swap} />}
      {swap != null && (
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <CancelButton swapState={swap} />
          <DebugPageSwitchBadge enabled={debug} setEnabled={setDebug} />
        </Box>
      )}
    </Paper>
  );
}
