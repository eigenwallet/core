import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Typography,
} from "@mui/material";
import { relaunch } from "@tauri-apps/plugin-process";
import { useAppSelector } from "store/hooks";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import ContactInfoBox from "renderer/components/other/ContactInfoBox";

export default function ContextErrorDialog() {
  const logs = useAppSelector((state) => state.logs.state.logs);
  const errorMessage = useAppSelector((state) =>
    state.rpc.status?.type === "error" ? state.rpc.status.error : null,
  );

  if (errorMessage === null) {
    return null;
  }

  return (
    <Dialog open={true} maxWidth="md" fullWidth disableEscapeKeyDown>
      <DialogTitle>Failed to start</DialogTitle>
      <DialogContent>
        <DialogContentText>
          Check the logs below for details. Try restarting the GUI. Reach out to
          the developers and the community if this continues.
        </DialogContentText>
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            gap: 2,
          }}
        >
          <Box sx={{ alignSelf: "center" }}>
            <ContactInfoBox />
          </Box>
          <ActionableMonospaceTextBox
            content={errorMessage}
            displayCopyIcon={true}
            enableQrCode={false}
          />
          <CliLogsBox label="Logs" logs={logs} minHeight="30vh" />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button variant="contained" onClick={() => relaunch()}>
          Restart GUI
        </Button>
      </DialogActions>
    </Dialog>
  );
}
