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

interface SeedPhraseModalProps {
  open: boolean;
  onClose: () => void;
  seedPhrase: string;
}

export default function SeedPhraseModal({
  open,
  onClose,
  seedPhrase,
}: SeedPhraseModalProps) {
  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Wallet Seed Phrase</DialogTitle>
      <DialogContent>
        <ActionableMonospaceTextBox
          content={seedPhrase}
          displayCopyIcon={true}
          enableQrCode={false}
          spoilerText="Press to reveal"
        />

        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ mt: 2, display: "block", fontStyle: "italic" }}
        >
          Keep this seed phrase safe and secure. Write it down on paper and
          store it in a safe place.
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
