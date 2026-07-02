import {
  Alert,
  Box,
  Checkbox,
  FormControlLabel,
  Typography,
} from "@mui/material";
import { useEffect, useState } from "react";
import {
  GetMoneroSeedResponse,
  GetRestoreHeightResponse,
} from "models/tauriModel";
import { getMoneroSeedAndRestoreHeight } from "renderer/rpc";
import { useIsMoneroWalletAvailable } from "store/hooks";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { PrivateKeyScamAlert } from "renderer/components/other/PrivateKeyWarning";
import CircularProgressWithSubtitle from "renderer/components/pages/swap/swap/components/CircularProgressWithSubtitle";

const SEED_FETCH_RETRY_INTERVAL_MS = 1000;
const SEED_FETCH_MAX_ATTEMPTS = 60;

/// Shown after a fresh wallet has been created so the user records their seed
/// before continuing. The seed only becomes readable once the newly created
/// Monero wallet has finished opening, so we wait for it before fetching.
export default function BackupSeedStep({
  onConfirmedChange,
}: {
  onConfirmedChange: (confirmed: boolean) => void;
}) {
  const moneroWalletAvailable = useIsMoneroWalletAvailable();
  const [seed, setSeed] = useState<
    [GetMoneroSeedResponse, GetRestoreHeightResponse] | null
  >(null);
  const [confirmed, setConfirmed] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!moneroWalletAvailable || seed !== null) {
      return;
    }

    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    const fetchSeed = () => {
      getMoneroSeedAndRestoreHeight()
        .then((result) => {
          if (!cancelled) setSeed(result);
        })
        .catch((e) => {
          if (cancelled) return;
          attempt += 1;
          if (attempt >= SEED_FETCH_MAX_ATTEMPTS) {
            console.error("Failed to read wallet seed for backup", e);
            setFailed(true);
            return;
          }
          timeout = setTimeout(fetchSeed, SEED_FETCH_RETRY_INTERVAL_MS);
        });
    };
    fetchSeed();

    return () => {
      cancelled = true;
      if (timeout !== undefined) clearTimeout(timeout);
    };
  }, [moneroWalletAvailable, seed]);

  if (failed) {
    return (
      <Alert severity="error">
        Could not read the wallet seed to back up. You can view it later from
        the wallet menu.
      </Alert>
    );
  }

  if (seed === null) {
    return (
      <CircularProgressWithSubtitle description="Preparing your new wallet…" />
    );
  }

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <PrivateKeyScamAlert />
      <Typography variant="body2" color="text.secondary">
        Write down your seed phrase and restore height. They are the only way to
        recover this wallet if you lose access to this device.
      </Typography>
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
      <FormControlLabel
        control={
          <Checkbox
            checked={confirmed}
            onChange={(e) => {
              setConfirmed(e.target.checked);
              onConfirmedChange(e.target.checked);
            }}
          />
        }
        label="I have written down my seed phrase and restore height"
      />
    </Box>
  );
}
