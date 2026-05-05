import {
  Box,
  Button,
  Dialog,
  DialogActions,
  Link,
  Paper,
  Tooltip,
  Typography,
} from "@mui/material";
import LocalOfferOutlinedIcon from "@mui/icons-material/LocalOfferOutlined";
import SwapHorizOutlinedIcon from "@mui/icons-material/SwapHorizOutlined";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
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
import { parseDateString } from "utils/parseUtils";
import { sortBy } from "lodash";

export type SwapWidgetMode = "offers" | "swaps";

export default function SwapWidget({ mode }: { mode: SwapWidgetMode }) {
  const matchingSwaps = useAppSelector((state) => {
    const filtered = Object.values(state.swap.swaps).filter((swap) => {
      // For released swaps the meaningful state is `prev` (curr is just the
      // generic "Released" marker). We keep them visible until acknowledged.
      const phaseEvent = swap.curr.type === "Released" ? swap.prev : swap.curr;
      if (phaseEvent == null) return false;
      return mode === "offers"
        ? isOfferPhase(phaseEvent)
        : !isOfferPhase(phaseEvent);
    });
    // Newest first. A swap may exist in the redux swap slice before its
    // SwapInfo row has been fetched - if any swap is missing info, leave the
    // list in its current order; the sort kicks in once everything is loaded.
    const swapInfos = state.rpc.state.swapInfos;
    if (swapInfos == null) return filtered;
    if (filtered.some((swap) => swapInfos[swap.swapId] == null)) {
      return filtered;
    }
    return sortBy(
      filtered,
      (swap) => -parseDateString(swapInfos[swap.swapId].start_date),
    );
  });
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
        {mode === "swaps" && matchingSwaps.length === 0 ? (
          <NoSwapsPlaceholder />
        ) : (
          visibleSwaps.map((swap) => (
            <SwapStatePanel key={swap?.swapId ?? "new-swap"} swap={swap} />
          ))
        )}
      </Box>
    </Box>
  );
}

function NoSwapsPlaceholder() {
  const navigate = useNavigate();

  return (
    <Paper
      elevation={3}
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 3,
        borderRadius: 2,
        padding: 6,
        minHeight: "40vh",
        textAlign: "center",
      }}
    >
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          width: 96,
          height: 96,
          borderRadius: "50%",
          backgroundColor: "action.hover",
          color: "text.secondary",
        }}
      >
        <SwapHorizOutlinedIcon sx={{ fontSize: 56 }} />
      </Box>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 0.75 }}>
        <Typography variant="h6">No swaps in progress</Typography>
        <Typography variant="body2" color="text.secondary">
          Browse live offers from makers to start your next swap.
        </Typography>
      </Box>
      <Button
        variant="contained"
        size="large"
        startIcon={<LocalOfferOutlinedIcon />}
        onClick={() => navigate("/offers")}
        sx={{ paddingX: 4, paddingY: 1.25 }}
      >
        Browse offers
      </Button>
    </Paper>
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
