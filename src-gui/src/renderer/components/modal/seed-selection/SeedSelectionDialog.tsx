import {
  Button,
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

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [selectedOption, setSelectedOption] = useState<string>("RandomSeed");
  const [customSeed, setCustomSeed] = useState<string>("");
  const [isSeedValid, setIsSeedValid] = useState<boolean>(false);
  const [isCheckingSeed, setIsCheckingSeed] = useState<boolean>(false);
  const approval = pendingApprovals[0]; // Handle the first pending approval

  useEffect(() => {
    let cancelled = false;
    if (selectedOption === "FromSeed" && customSeed.trim()) {
      setIsCheckingSeed(true);
      checkSeed(customSeed.trim())
        .then((valid) => {
          if (!cancelled) setIsSeedValid(valid);
        })
        .catch(() => {
          if (!cancelled) setIsSeedValid(false);
        })
        .finally(() => {
          if (!cancelled) setIsCheckingSeed(false);
        });
    } else {
      setIsSeedValid(false);
      setIsCheckingSeed(false);
    }
    return () => {
      cancelled = true;
    };
  }, [customSeed, selectedOption]);

  const handleClose = async (accept: boolean) => {
    if (!approval) return;

    if (accept) {
      const seedChoice =
        selectedOption === "RandomSeed"
          ? { type: "RandomSeed" }
          : { type: "FromSeed", content: { seed: customSeed } };

      await resolveApproval(approval.request_id, seedChoice);
    } else {
      // On reject, just close without approval
      await resolveApproval(approval.request_id, { type: "RandomSeed" });
    }
  };

  if (!approval) {
    return null;
  }

  return (
    <Dialog open={true} maxWidth="sm" fullWidth>
      <DialogTitle>Seed Selection</DialogTitle>
      <DialogContent>
        <Typography variant="body1" sx={{ mb: 2 }}>
          Choose how to handle the wallet seed:
        </Typography>
        
        <FormControl component="fieldset">
          <RadioGroup
            value={selectedOption}
            onChange={(e) => setSelectedOption(e.target.value)}
          >
            <FormControlLabel
              value="RandomSeed"
              control={<Radio />}
              label="Generate a random seed (recommended)"
            />
            <FormControlLabel
              value="FromSeed"
              control={<Radio />}
              label="Use custom seed"
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
            error={!isSeedValid}
            helperText={isCheckingSeed ? "Checking seed..." : (isSeedValid ? "Seed is valid" : "Seed is invalid")}
          />
        )}
      </DialogContent>
      <DialogActions>
        <Button 
          onClick={() => handleClose(true)} 
          variant="contained"
          disabled={
            isCheckingSeed ||
            selectedOption === "FromSeed"
              ? (!customSeed.trim() || !isSeedValid || isCheckingSeed)
              : false 
          }
        >
          Confirm
        </Button>
      </DialogActions>
    </Dialog>
  );
} 