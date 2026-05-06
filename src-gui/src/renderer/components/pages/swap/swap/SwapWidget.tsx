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
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";
import LocalOfferOutlinedIcon from "@mui/icons-material/LocalOfferOutlined";
import SwapHorizOutlinedIcon from "@mui/icons-material/SwapHorizOutlined";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { SwapState } from "models/storeModel";
import { GetSwapInfoResponseExt, isOfferPhase } from "models/tauriModelExt";
import {
  useAppSelector,
  useIdleResumableSwapInfos,
  useSwapInfo,
} from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import CancelButton from "./CancelButton";
import TimelockButton from "./TimelockButton";
import RetryBackoffAlert from "./RetryBackoffAlert";
import SwapStateStepper from "renderer/components/modal/swap/SwapStateStepper";
import DebugPageSwitchBadge from "renderer/components/modal/swap/pages/DebugPageSwitchBadge";
import DebugPage from "renderer/components/modal/swap/pages/DebugPage";
import MockSwapControls from "renderer/components/modal/swap/pages/MockSwapControls";
import ClickToCopy from "renderer/components/other/ClickToCopy";
import BitcoinIcon from "renderer/components/icons/BitcoinIcon";
import MoneroIcon from "renderer/components/icons/MoneroIcon";
import { SatsAmount, PiconeroAmount } from "renderer/components/other/Units";
import TruncatedText from "renderer/components/other/TruncatedText";
import { SwapResumeButton } from "renderer/components/pages/history/table/HistoryRowActions";
import { swapIdColor } from "utils/swapColor";
import { parseDateString } from "utils/parseUtils";
import { sortBy } from "lodash";

export type SwapWidgetMode = "offers" | "swaps";

type SwapsListEntry =
  | { kind: "active"; state: SwapState; swapId: string }
  | { kind: "idle"; info: GetSwapInfoResponseExt; swapId: string };

export default function SwapWidget({ mode }: { mode: SwapWidgetMode }) {
  useRedirectOnOfferAccepted(mode);
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

  // Idle resumable swaps belong only on the "swaps" tab — they are by
  // definition past the offer phase (funds locked).
  const idleResumableSwaps = useIdleResumableSwapInfos();
  const swapInfos = useAppSelector((state) => state.rpc.state.swapInfos);

  const combinedEntries: SwapsListEntry[] = (() => {
    if (mode !== "swaps") {
      return matchingSwaps.map((s) => ({
        kind: "active" as const,
        state: s,
        swapId: s.swapId,
      }));
    }
    const entries: SwapsListEntry[] = matchingSwaps.map((s) => ({
      kind: "active" as const,
      state: s,
      swapId: s.swapId,
    }));
    // Dedupe defensively: a swap that has any entry in the redux swap slice
    // (running, retry-backoff, or terminally Released) is already covered by
    // an active panel, so we must not also surface an "idle resumable" panel
    // for it. The hook itself already filters these out, but the active list
    // also includes redux entries that *don't* round-trip through the hook
    // filter (e.g. swaps still being driven by an in-flight retry banner),
    // so we re-check here.
    const activeIds = new Set(entries.map((e) => e.swapId));
    for (const info of idleResumableSwaps) {
      if (activeIds.has(info.swap_id)) continue;
      entries.push({ kind: "idle", info, swapId: info.swap_id });
    }
    if (swapInfos == null) return entries;
    if (entries.some((e) => swapInfos[e.swapId] == null)) return entries;
    return sortBy(
      entries,
      (e) => -parseDateString(swapInfos[e.swapId].start_date),
    );
  })();

  // The offers tab shows the InitPage placeholder when no offer is in flight,
  // so the user can start a new swap. The swaps tab simply renders nothing
  // when there is no in-progress or resumable swap to show.
  const showOfferPlaceholder =
    mode === "offers" && combinedEntries.length === 0;

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
        {mode === "swaps" && combinedEntries.length === 0 ? (
          <NoSwapsPlaceholder />
        ) : showOfferPlaceholder ? (
          <SwapStatePanel swap={null} />
        ) : (
          combinedEntries.map((entry) =>
            entry.kind === "active" ? (
              <SwapStatePanel key={entry.swapId} swap={entry.state} />
            ) : (
              <IdleResumableSwapPanel key={entry.swapId} swap={entry.info} />
            ),
          )
        )}
      </Box>
    </Box>
  );
}

// When a swap on the offers tab transitions out of the offer phase (i.e. the
// user accepted an offer and we're now locking funds), pull them over to
// /swap so they don't have to navigate manually. We track which swap ids
// we've already redirected for so a re-render can't bounce them back.
function useRedirectOnOfferAccepted(mode: SwapWidgetMode) {
  const navigate = useNavigate();
  const swaps = useAppSelector((state) => state.swap.swaps);
  const redirected = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (mode !== "offers") return;
    for (const swap of Object.values(swaps)) {
      if (redirected.current.has(swap.swapId)) continue;
      if (swap.prev == null) continue;
      if (!isOfferPhase(swap.prev)) continue;
      if (isOfferPhase(swap.curr)) continue;
      if (swap.curr.type === "Released") continue;
      redirected.current.add(swap.swapId);
      navigate("/swap");
      return;
    }
  }, [swaps, mode, navigate]);
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
        borderRadius: 2,
        // Clip TimelockButton's alert to the paper's rounded corners so it
        // visually attaches to the top of the box rather than floating
        // inside it.
        overflow: "hidden",
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
      {swap != null && <TimelockButton swapId={swap.swapId} />}
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          padding: 2,
        }}
      >
        {swap != null && <SwapAmountHeader swapId={swap.swapId} />}
        {swap != null && <RetryBackoffAlert swapId={swap.swapId} />}
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
              // Edge-to-edge top divider matching the header's bottom border.
              mx: -2,
              mb: -2,
              px: 2,
              py: 1,
              borderTop: 1,
              borderColor: "divider",
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
      </Box>
    </Paper>
  );
}

// A resumable swap that is not currently being driven by a state machine —
// shown alongside active swaps on the Swaps page so the user can resume it
// without navigating to the History view. Once the user clicks Resume the
// state machine starts emitting progress events and the swap migrates to a
// regular `SwapStatePanel`.
function IdleResumableSwapPanel({ swap }: { swap: GetSwapInfoResponseExt }) {
  return (
    <Paper
      elevation={3}
      sx={{
        display: "flex",
        flexDirection: "column",
        borderRadius: 2,
        overflow: "hidden",
        borderTop: `2px solid ${swapIdColor(swap.swap_id, 0.85)}`,
      }}
    >
      <TimelockButton swapId={swap.swap_id} />
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          padding: 2,
        }}
      >
        <SwapAmountHeader swapId={swap.swap_id} />
        <Box
          sx={{
            display: "flex",
            minHeight: "30vh",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 2,
            textAlign: "center",
          }}
        >
          <Typography variant="h6">Swap is suspended</Typography>
          <SwapResumeButton swap={swap}>Resume</SwapResumeButton>
        </Box>
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            mx: -2,
            mb: -2,
            px: 2,
            py: 1,
            borderTop: 1,
            borderColor: "divider",
          }}
        >
          <ClickToCopy content={swap.swap_id}>
            <Tooltip title={swap.swap_id}>
              <Link
                component="span"
                variant="caption"
                color="text.secondary"
                underline="always"
                sx={{
                  fontFamily: "monospace",
                  textDecorationColor: swapIdColor(swap.swap_id),
                  textDecorationThickness: 2,
                  textUnderlineOffset: 3,
                }}
              >
                <TruncatedText limit={8} truncateMiddle>
                  {swap.swap_id}
                </TruncatedText>
              </Link>
            </Tooltip>
          </ClickToCopy>
        </Box>
      </Box>
    </Paper>
  );
}

// Header showing the swap's BTC -> XMR amounts. Reads from the swapInfo
// (which is fetched lazily after the swap_id appears), so we render nothing
// until the amounts are known to avoid a flash of "undefined -> undefined".
function SwapAmountHeader({ swapId }: { swapId: string }) {
  const swapInfo = useSwapInfo(swapId);
  if (swapInfo == null) return null;

  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 0.75,
        color: "text.secondary",
        // Extend edge-to-edge by undoing the parent Box's horizontal
        // padding, then re-adding our own; gives a clean full-width divider.
        mx: -2,
        mt: -2,
        px: 2,
        py: 1,
        borderBottom: 1,
        borderColor: "divider",
      }}
    >
      <BitcoinIcon sx={{ fontSize: "1rem" }} />
      <Typography variant="body2" component="span">
        <SatsAmount amount={swapInfo.btc_amount} />
      </Typography>
      <ArrowForwardIcon sx={{ fontSize: "1rem" }} />
      <MoneroIcon sx={{ fontSize: "1rem" }} />
      <Typography variant="body2" component="span">
        <PiconeroAmount amount={swapInfo.xmr_amount} />
      </Typography>
    </Box>
  );
}
