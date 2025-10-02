import { Box, DialogContentText } from "@mui/material";
import { useActiveSwapLogs } from "store/hooks";
import CliLogsBox from "../../../other/RenderedCliLog";

export default function DebugPage() {
  const logs = useActiveSwapLogs();

  return (
    <Box sx={{ padding: 2, display: "flex", flexDirection: "column", gap: 2 }}>
      <DialogContentText>
        <Box
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "8px",
          }}
        >
          <CliLogsBox
            minHeight="min(20rem, 70vh)"
            logs={logs}
            label="Logs relevant to the swap (only current session)"
          />
        </Box>
      </DialogContentText>
    </Box>
  );
}
