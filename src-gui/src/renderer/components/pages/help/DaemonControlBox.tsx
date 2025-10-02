import { Box } from "@mui/material";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useAppSelector } from "store/hooks";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import { getDataDir } from "renderer/rpc";
import { relaunch } from "@tauri-apps/plugin-process";
import RotateLeftIcon from "@mui/icons-material/RotateLeft";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

export default function DaemonControlBox() {
  const logs = useAppSelector((s) => s.logs.state.logs);

  const stringifiedDaemonStatus = useAppSelector((s) => {
    if (s.rpc.status === null) {
      return "not started";
    }
    if (s.rpc.status.type === "error") {
      return "failed";
    }
    return "running";
  });

  return (
    <InfoBox
      id="daemon-control-box"
      title={`Daemon Controller (${stringifiedDaemonStatus})`}
      mainContent={
        <CliLogsBox label="Logs (current session only)" logs={logs} />
      }
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
