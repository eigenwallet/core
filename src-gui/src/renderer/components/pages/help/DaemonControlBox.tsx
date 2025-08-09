import { Box } from "@mui/material";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useAppSelector, useOrderedLogPairs } from "store/hooks";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import { CircularProgress, Typography } from "@mui/material";
import { useEffect, useMemo, useRef, useState } from "react";
import { store } from "renderer/store/storeRenderer";
import { requestLogsWindow } from "store/features/logsSlice";
import { getDataDir, initializeContext } from "renderer/rpc";
import { relaunch } from "@tauri-apps/plugin-process";
import RotateLeftIcon from "@mui/icons-material/RotateLeft";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { TauriContextStatusEvent } from "models/tauriModel";

export default function DaemonControlBox() {
  const logPairs = useOrderedLogPairs();

  // The daemon can be manually started if it has failed or if it has not been started yet
  const canContextBeManuallyStarted = useAppSelector(
    (s) =>
      s.rpc.status === TauriContextStatusEvent.Failed || s.rpc.status === null,
  );
  const isContextInitializing = useAppSelector(
    (s) => s.rpc.status === TauriContextStatusEvent.Initializing,
  );

  const stringifiedDaemonStatus = useAppSelector(
    (s) => s.rpc.status ?? "not started",
  );

  const inflight = useAppSelector((s) => s.logs.state.inflight_log_fetching);
  const baseIndex = useAppSelector((s) => s.logs.state.baseIndex) ?? 0;
  const endIndex = useAppSelector((s) => s.logs.state.endIndex) ?? 0;
  const firstIndex = logPairs[0]?.[0] ?? baseIndex;
  const lastIndex = logPairs[logPairs.length - 1]?.[0] ?? 0;
  const [nearTop, setNearTop] = useState(false);
  const [nearBottom, setNearBottom] = useState(false);
  // Keep fetch pages small to avoid overfetch
  const CHUNK = 200;
  const loadingAdornment = useMemo(() => {
    if (inflight.length === 0) return null;
    const [start, end] = inflight[inflight.length - 1];
    return (
      <Box sx={{ display: "flex", alignItems: "center", gap: 1, px: 1 }}>
        <CircularProgress size={14} />
        <Typography variant="caption">
          fetching logs {start}...{end}
        </Typography>
      </Box>
    );
  }, [inflight]);

  return (
    <InfoBox
      id="daemon-control-box"
      title={`Daemon Controller (${stringifiedDaemonStatus})`}
      mainContent={
        <CliLogsBox
          label="Logs (current session only)"
          logs={logPairs.map(([, log]) => log)}
          logPairs={logPairs}
          topAdornment={loadingAdornment}
          onReachTop={() => {
            setNearTop(true);
            setNearBottom(false);
            const currentBase =
              store.getState().logs.state.logs[0]?.[0] ?? baseIndex;
            const gap = Math.max(0, currentBase - baseIndex);
            const request = Math.min(CHUNK, gap);
            if (request > 0) {
              const start = currentBase - request;
              store.dispatch(requestLogsWindow({ start, end: currentBase }));
            }
          }}
          onReachBottom={() => {
            setNearBottom(true);
            setNearTop(false);
            const state = store.getState().logs.state;
            const tailIndex = state.endIndex ?? 0;
            const currentLast = state.logs[state.logs.length - 1]?.[0] ?? 0;
            const gap = Math.max(0, tailIndex - (currentLast + 1));
            const request = Math.min(CHUNK, gap);
            if (request > 0) {
              const start = currentLast + 1;
              const end = start + request;
              store.dispatch(requestLogsWindow({ start, end }));
            }
          }}
        />
      }
      additionalContent={
        <Box sx={{ display: "flex", gap: 1, alignItems: "center" }}>
          <PromiseInvokeButton
            variant="contained"
            endIcon={<PlayArrowIcon />}
            onInvoke={initializeContext}
            requiresContext={false}
            disabled={!canContextBeManuallyStarted}
            isLoadingOverride={isContextInitializing}
            displayErrorSnackbar
          >
            Start Daemon
          </PromiseInvokeButton>
          <PromiseInvokeButton
            variant="contained"
            endIcon={<RotateLeftIcon />}
            onInvoke={relaunch}
            requiresContext={false}
            displayErrorSnackbar
          >
            Restart GUI
          </PromiseInvokeButton>
          <PromiseInvokeButton
            endIcon={<FolderOpenIcon />}
            isIconButton
            requiresContext={false}
            size="small"
            tooltipTitle="Open the data directory in your file explorer"
            onInvoke={async () => {
              const dataDir = await getDataDir();
              await revealItemInDir(dataDir);
            }}
          />
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}

function isRangeInflight(
  inflight: [number, number][],
  start: number,
  end: number,
): boolean {
  return inflight.some(([s, e]) => !(end <= s || start >= e));
}

// Continuous prefetching while user is at top/bottom
export function DaemonControlBoxPrefetchEffects({
  nearTop,
  nearBottom,
}: {
  nearTop: boolean;
  nearBottom: boolean;
}) {
  const inflight = useAppSelector((s) => s.logs.state.inflight_log_fetching);
  const baseIndex = useAppSelector((s) => s.logs.state.baseIndex) ?? 0;
  const endIndex = useAppSelector((s) => s.logs.state.endIndex) ?? 0;
  const logPairs = useOrderedLogPairs();
  const firstIndex = logPairs[0]?.[0] ?? baseIndex;
  const lastIndex = logPairs[logPairs.length - 1]?.[0] ?? 0;
  const CHUNK = 200;

  useEffect(() => {
    if (!nearTop) return;
    if (firstIndex <= baseIndex) return;
    const gap = Math.max(0, firstIndex - baseIndex);
    const request = Math.min(CHUNK, gap);
    if (request > 0) {
      const start = firstIndex - request;
      if (!isRangeInflight(inflight, start, firstIndex)) {
        store.dispatch(requestLogsWindow({ start, end: firstIndex }));
      }
    }
  }, [nearTop, firstIndex, baseIndex, inflight]);

  useEffect(() => {
    if (!nearBottom) return;
    if (lastIndex >= endIndex - 1) return;
    const start = lastIndex + 1;
    const gap = Math.max(0, endIndex - start);
    const request = Math.min(CHUNK, gap);
    if (request > 0) {
      const end = start + request;
      if (!isRangeInflight(inflight, start, end)) {
        store.dispatch(requestLogsWindow({ start, end }));
      }
    }
  }, [nearBottom, lastIndex, endIndex, inflight]);

  return null;
}
