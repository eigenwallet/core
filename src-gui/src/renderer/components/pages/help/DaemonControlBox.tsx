import { Box, Paper, Typography } from "@mui/material";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import VisibilityIcon from "@mui/icons-material/Visibility";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useAppSelector } from "store/hooks";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import { getDataDir } from "renderer/rpc";
import { relaunch } from "@tauri-apps/plugin-process";
import RotateLeftIcon from "@mui/icons-material/RotateLeft";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { ContextStatusType } from "store/features/rpcSlice";
import { useEffect, useRef, useState } from "react";

const LOGS_HIDE_AFTER_OUT_OF_VIEW_MS = 10_000;

function DaemonLogs() {
  const logs = useAppSelector((s) => s.logs.state.logs);

  return <CliLogsBox label="Logs (current session only)" logs={logs} />;
}

function LazyDaemonLogs() {
  const [active, setActive] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!active || containerRef.current === null) {
      return;
    }

    let hideTimeout: ReturnType<typeof setTimeout> | undefined;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting) {
        clearTimeout(hideTimeout);
        hideTimeout = undefined;
      } else if (hideTimeout === undefined) {
        hideTimeout = setTimeout(
          () => setActive(false),
          LOGS_HIDE_AFTER_OUT_OF_VIEW_MS,
        );
      }
    });
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      clearTimeout(hideTimeout);
    };
  }, [active]);

  return (
    <Box ref={containerRef} sx={{ width: "100%" }}>
      {active ? (
        <DaemonLogs />
      ) : (
        <Paper
          variant="outlined"
          onClick={() => setActive(true)}
          sx={{
            minHeight: "10rem",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 1,
            cursor: "pointer",
            backgroundColor: "action.hover",
            color: "text.secondary",
          }}
        >
          <VisibilityIcon />
          <Typography variant="subtitle2">Click to show logs</Typography>
        </Paper>
      )}
    </Box>
  );
}

export default function DaemonControlBox() {
  const stringifiedDaemonStatus = useAppSelector((s) => {
    if (s.rpc.status === null) {
      return "not started";
    }
    if (s.rpc.status.type === ContextStatusType.Error) {
      return "failed";
    }
    return "running";
  });

  return (
    <InfoBox
      id="daemon-control-box"
      title={`Daemon Controller (${stringifiedDaemonStatus})`}
      mainContent={<LazyDaemonLogs />}
      additionalContent={
        <Box sx={{ display: "flex", gap: 1, alignItems: "center" }}>
          <PromiseInvokeButton
            variant="contained"
            endIcon={<RotateLeftIcon />}
            onInvoke={relaunch}
            contextRequirement={false}
            displayErrorSnackbar
          >
            Restart GUI
          </PromiseInvokeButton>
          <PromiseInvokeButton
            endIcon={<FolderOpenIcon />}
            isIconButton
            contextRequirement={false}
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
