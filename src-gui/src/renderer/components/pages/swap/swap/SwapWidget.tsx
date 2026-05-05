import {
  Box,
  Button,
  Dialog,
  DialogActions,
  Link,
  Paper,
  Tooltip,
} from "@mui/material";
import { useState } from "react";
import { SwapState } from "models/storeModel";
import { isOfferPhase } from "models/tauriModelExt";
import { useAppSelector } from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import MockSwapControls from "renderer/components/modal/swap/pages/MockSwapControls";
import ClickToCopy from "renderer/components/other/ClickToCopy";
import TruncatedText from "renderer/components/other/TruncatedText";
import { swapIdColor } from "utils/swapColor";

export type SwapWidgetMode = "offers" | "swaps";

export default function SwapWidget({ mode }: { mode: SwapWidgetMode }) {
  const matchingSwaps = useAppSelector((state) =>
    Object.values(state.swap.swaps).filter((swap) => {
      if (swap.curr.type === "Released") return false;
      return mode === "offers"
        ? isOfferPhase(swap.curr)
        : !isOfferPhase(swap.curr);
    }),
  );
  // The offers tab shows the InitPage placeholder when no offer is in flight,
  // so the user can start a new swap. The swaps tab simply renders nothing
  // when there is no in-progress swap to show.
  const visibleSwaps: (SwapState | null)[] =
    mode === "offers" && matchingSwaps.length === 0 ? [null] : matchingSwaps;

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
        {visibleSwaps.map((swap) => (
          <SwapStatePanel key={swap?.swapId ?? "new-swap"} swap={swap} />
        ))}
      </Box>
    </Box>
  );
}

function SwapStatePanel({ swap }: { swap: SwapState | null }) {
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
        ...(swap != null && {
          borderTop: `2px solid ${swapIdColor(swap.swapId, 0.85)}`,
        }),
      }}
    >
      {swap != null && (
        <>
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
          <ClickToCopy content={swap.swapId}>
            <Tooltip title={swap.swapId}>
              <Link
                component="span"
                variant="caption"
                color="text.secondary"
                underline="always"
                sx={{
                  fontFamily: "monospace",
                  textDecorationColor: swapIdColor(swap.swapId),
                  textDecorationThickness: 2,
                  textUnderlineOffset: 3,
                }}
              >
                <TruncatedText limit={8} truncateMiddle>
                  {swap.swapId}
                </TruncatedText>
              </Link>
            </Tooltip>
          </ClickToCopy>
          <DebugPageSwitchBadge enabled={debug} setEnabled={setDebug} />
        </Box>
      )}
    </Paper>
  );
}
