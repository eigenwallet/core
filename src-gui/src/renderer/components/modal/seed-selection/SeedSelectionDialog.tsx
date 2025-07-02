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
import { useState } from "react";
import { usePendingSeedSelectionApproval } from "store/hooks";
import { resolveApproval } from "renderer/rpc";

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [selectedOption, setSelectedOption] = useState<string>("RandomSeed");
  const [customSeed, setCustomSeed] = useState<string>("");

  const approval = pendingApprovals[0]; // Handle the first pending approval

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
          />
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={() => handleClose(false)}>Cancel</Button>
        <Button 
          onClick={() => handleClose(true)} 
          variant="contained"
          disabled={selectedOption === "FromSeed" && !customSeed.trim()}
        >
          Confirm
        </Button>
      </DialogActions>
    </Dialog>
  );
} 