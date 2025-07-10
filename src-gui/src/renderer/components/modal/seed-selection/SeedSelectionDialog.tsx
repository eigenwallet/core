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
  Button,
  Box,
} from "@mui/material";
import { useState, useEffect } from "react";
import { usePendingSeedSelectionApproval } from "store/hooks";
import { resolveApproval, checkSeed } from "renderer/rpc";
import { SeedChoice } from "models/tauriModel";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { open } from "@tauri-apps/plugin-dialog";

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [selectedOption, setSelectedOption] = useState<
    SeedChoice["type"] | undefined
  >("RandomSeed");
  const [customSeed, setCustomSeed] = useState<string>("");
  const [isSeedValid, setIsSeedValid] = useState<boolean>(false);
  const [walletPath, setWalletPath] = useState<string>("");

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

  const selectWalletFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
    });

    if (selected) {
      setWalletPath(selected);
    }
  };

  const accept = async () => {
    if (!approval)
      throw new Error("No approval request found for seed selection");

    const seedChoice: SeedChoice =
      selectedOption === "RandomSeed"
        ? { type: "RandomSeed" }
        : selectedOption === "FromSeed"
          ? { type: "FromSeed", content: { seed: customSeed } }
          : { type: "FromWalletPath", content: { wallet_path: walletPath } };

    await resolveApproval<SeedChoice>(approval.request_id, seedChoice);
  };

  if (!approval) {
    return null;
  }

  // Disable the button if the user is restoring from a seed and the seed is invalid
  // or if selecting wallet path and no path is selected
  const isDisabled =
    selectedOption === "FromSeed"
      ? customSeed.trim().length === 0 || !isSeedValid
      : selectedOption === "FromWalletPath"
        ? !walletPath
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
            <FormControlLabel
              value="FromWalletPath"
              control={<Radio />}
              label="Open existing wallet file"
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

        {selectedOption === "FromWalletPath" && (
          <Box sx={{ mt: 2 }}>
            <Typography variant="body2" sx={{ mb: 1 }}>
              Select your existing Monero wallet file:
            </Typography>
            <Box sx={{ display: "flex", gap: 1, alignItems: "center" }}>
              <TextField
                fullWidth
                label="Wallet file path"
                value={walletPath || ""}
                placeholder="Select a wallet file..."
                InputProps={{
                  readOnly: true,
                }}
              />
              <Button
                variant="outlined"
                onClick={selectWalletFile}
                sx={{ minWidth: "120px" }}
                size="large"
              >
                Browse
              </Button>
            </Box>
          </Box>
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
