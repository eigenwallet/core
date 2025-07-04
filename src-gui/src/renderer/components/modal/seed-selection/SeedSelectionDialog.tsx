import {
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  FormControlLabel,
  Radio,
  RadioGroup,
  TextField,
  Typography,
} from "@mui/material";
import { useState, useEffect } from "react";
import { usePendingSeedSelectionApproval } from "store/hooks";
import { resolveApproval, checkSeed } from "renderer/rpc";
import { SeedChoice } from "models/tauriModel";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [selectedOption, setSelectedOption] = useState<
    SeedChoice["type"] | undefined
  >("RandomSeed");
  const [customSeed, setCustomSeed] = useState<string>("");
  const [isSeedValid, setIsSeedValid] = useState<boolean>(false);

  const approval = pendingApprovals[0];

  useEffect(() => {
    if (selectedOption === "FromSeed" && customSeed.trim()) {
      checkSeed(customSeed.trim())
        .then((valid) => {
          setIsSeedValid(valid);
        })
        .catch(() => {
          setIsSeedValid(false);
        });
    } else {
      setIsSeedValid(false);
    }
  }, [customSeed, selectedOption]);

  const accept = async () => {
    if (!approval)
      throw new Error("No approval request found for seed selection");

    const seedChoice: SeedChoice =
      selectedOption === "RandomSeed"
        ? { type: "RandomSeed" }
        : { type: "FromSeed", content: { seed: customSeed } };

    await resolveApproval<SeedChoice>(approval.request_id, seedChoice);
  };

  if (!approval) {
    return null;
  }

  // Disable the button if the user is restoring from a seed and the seed is invalid
  const isDisabled =
    selectedOption === "FromSeed"
      ? customSeed.trim().length === 0 || !isSeedValid
      : false;

  return (
    <Dialog open={true} maxWidth="sm" fullWidth>
      <DialogTitle>Monero Wallet</DialogTitle>
      <DialogContent>
        <Typography variant="body1" sx={{ mb: 2 }}>
          Choose what seed to use for the wallet.
        </Typography>

        <FormControl component="fieldset">
          <RadioGroup
            value={selectedOption}
            onChange={(e) =>
              setSelectedOption(e.target.value as SeedChoice["type"])
            }
          >
            <FormControlLabel
              value="RandomSeed"
              control={<Radio />}
              label="Create a new wallet"
            />
            <FormControlLabel
              value="FromSeed"
              control={<Radio />}
              label="Restore wallet from seed"
            />
          </RadioGroup>
        </FormControl>

        {selectedOption === "FromSeed" && (
          <TextField
            fullWidth
            multiline
            rows={3}
            label="Enter your seed phrase"
            value={customSeed}
            onChange={(e) => setCustomSeed(e.target.value)}
            sx={{ mt: 2 }}
            placeholder="Enter your Monero 25 words seed phrase..."
            error={!isSeedValid && customSeed.length > 0}
            helperText={
              isSeedValid
                ? "Seed is valid"
                : customSeed.length > 0
                  ? "Seed is invalid"
                  : ""
            }
          />
        )}
      </DialogContent>
      <DialogActions>
        <PromiseInvokeButton
          onInvoke={accept}
          variant="contained"
          disabled={isDisabled}
          requiresContext={false}
        >
          Continue
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
