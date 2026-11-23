import { Box, TextField, Typography } from "@mui/material";
import { open } from "@tauri-apps/plugin-dialog";
import SearchIcon from "@mui/icons-material/Search";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

export default function NameLocationStep({
  name,
  setName,
  directory,
  setDirectory,
}: {
  name: string;
  setName: (name: string) => void;
  directory: string;
  setDirectory: (directory: string) => void;
}) {
  const selectDirectory = async () => {
    const selected = await open({ multiple: false, directory: true });
    if (selected) setDirectory(selected);
  };

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <Typography variant="body2" color="text.secondary">
        Choose a name for the wallet file and where to store it on this device.
      </Typography>
      <TextField
        fullWidth
        autoFocus
        label="Wallet name"
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="my-wallet"
        error={name.length > 0 && name.trim().length === 0}
        helperText={
          name.length > 0 && name.trim().length === 0
            ? "Enter a wallet name"
            : ""
        }
      />
      <Box sx={{ display: "flex", gap: 1, alignItems: "center" }}>
        <TextField
          fullWidth
          label="Save location"
          value={directory}
          placeholder="Select a folder..."
          InputProps={{ readOnly: true }}
        />
        <PromiseInvokeButton
          variant="outlined"
          onInvoke={selectDirectory}
          contextRequirement={false}
          displayErrorSnackbar
          sx={{ minWidth: "120px", height: "56px" }}
          startIcon={<SearchIcon />}
        >
          Browse
        </PromiseInvokeButton>
      </Box>
    </Box>
  );
}
