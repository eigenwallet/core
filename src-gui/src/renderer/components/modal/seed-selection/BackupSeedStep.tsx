import { Box, Checkbox, FormControlLabel, Typography } from "@mui/material";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { PrivateKeyScamAlert } from "renderer/components/other/PrivateKeyWarning";

/// Shown while the backend blocks startup on its SeedBackup approval: the
/// freshly created wallet's seed is displayed once so the user records it
/// before continuing.
export default function BackupSeedStep({
  seed,
  restoreHeight,
  confirmed,
  onConfirmedChange,
}: {
  seed: string;
  restoreHeight: number;
  confirmed: boolean;
  onConfirmedChange: (confirmed: boolean) => void;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <PrivateKeyScamAlert />
      <Typography variant="body2" color="text.secondary">
        Write down your seed phrase and restore height. They are the only way to
        recover this wallet if you lose access to this device.
      </Typography>
      <ActionableMonospaceTextBox
        content={seed}
        displayCopyIcon={true}
        enableQrCode={false}
        spoilerText="Press to reveal"
      />
      <ActionableMonospaceTextBox
        content={restoreHeight.toString()}
        displayCopyIcon={true}
        enableQrCode={false}
      />
      <FormControlLabel
        control={
          <Checkbox
            checked={confirmed}
            onChange={(e) => onConfirmedChange(e.target.checked)}
          />
        }
        label="I have written down my seed phrase and restore height"
      />
    </Box>
  );
}
