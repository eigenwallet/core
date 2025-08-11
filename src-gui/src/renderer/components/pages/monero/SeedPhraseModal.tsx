import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Typography,
} from "@mui/material";
import ActionableMonospaceTextBox from "../../other/ActionableMonospaceTextBox";
import {
  GetMoneroSeedResponse,
  GetRestoreHeightResponse,
} from "models/tauriModel";

interface SeedPhraseModalProps {
  onClose: () => void;
  seed: [GetMoneroSeedResponse, GetRestoreHeightResponse] | null;
}

export default function SeedPhraseModal({
  onClose,
  seed,
}: SeedPhraseModalProps) {
  if (seed === null) {
    return null;
  }

  return (
    <Dialog open={true} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Wallet Seed Phrase</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <ActionableMonospaceTextBox
            content={seed[0].seed}
            displayCopyIcon={true}
            enableQrCode={false}
            spoilerText="Press to reveal"
          />
          <ActionableMonospaceTextBox
            content={seed[1].height.toString()}
            displayCopyIcon={true}
            enableQrCode={false}
          />
        </Box>

        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ mt: 2, display: "block", fontStyle: "italic" }}
        >
          Keep this seed phrase safe and secure. Write it down on paper and
          store it in a safe place. Keep the restore height in mind when you
          restore your wallet on another device.
        </Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} variant="contained">
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}
