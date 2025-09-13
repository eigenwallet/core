import {
  Alert,
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
import { useEffect, useState } from "react";
import { getMoneroSeedAndRestoreHeight } from "renderer/rpc";

interface SeedPhraseModalProps {
  onClose: () => void;
  open: boolean;
}

interface Info {
  seed: string;
  restoreHeight: number;
}

export default function SeedPhraseModal({
  onClose,
  open,
}: SeedPhraseModalProps) {
  const [info, setInfo] = useState<Info | null>(null);

  useEffect(() => {
    getMoneroSeedAndRestoreHeight().then(([seed, height]) => {
      setInfo({ seed: seed.seed, restoreHeight: height.height });
    });
  }, []);

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Export your Monero wallet's seed</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <Alert severity="info">
            Never reveal your seed phrase to <i>anyone</i>. The developers will
            never ask for your seed.
          </Alert>

          <Typography variant="body1">Seed phrase</Typography>

          <ActionableMonospaceTextBox
            content={info == null ? "...loading..." : info.seed}
            displayCopyIcon={true}
            enableQrCode={false}
            spoilerText="Press to reveal"
          />

          <Typography variant="caption">
            The seed phrase of your wallet is equivalent to the secret key.
          </Typography>

          <Typography variant="body1">Restore Block Height</Typography>

          <ActionableMonospaceTextBox
            content={
              info == null ? "...loading..." : info.restoreHeight.toString()
            }
            displayCopyIcon={true}
            enableQrCode={false}
          />

          <Typography variant="caption">
            The restore height will help other wallets determine which parts of
            the blockchain to scan for funds.
          </Typography>
        </Box>

        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ mt: 2, display: "block", fontStyle: "italic" }}
        ></Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} variant="outlined">
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}
