import { ListItemIcon, MenuItem, Typography } from "@mui/material";
import { Key as KeyIcon } from "@mui/icons-material";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { getMoneroSeed } from "renderer/rpc";
import { GetMoneroSeedResponse } from "models/tauriModel";

interface SeedPhraseButtonProps {
  onMenuClose: () => void;
  onSeedPhraseSuccess: (response: GetMoneroSeedResponse) => void;
}

export default function SeedPhraseButton({
  onMenuClose,
  onSeedPhraseSuccess,
}: SeedPhraseButtonProps) {
  const handleSeedPhraseSuccess = (response: GetMoneroSeedResponse) => {
    onSeedPhraseSuccess(response);
    onMenuClose();
  };

  return (
    <MenuItem component="div">
      <PromiseInvokeButton
        onInvoke={getMoneroSeed}
        onSuccess={handleSeedPhraseSuccess}
        displayErrorSnackbar={true}
        variant="text"
        sx={{
          justifyContent: "flex-start",
          textTransform: "none",
          padding: 0,
          minHeight: "auto",
          width: "100%",
          color: "text.primary",
        }}
      >
        <ListItemIcon>
          <KeyIcon />
        </ListItemIcon>
        <Typography>Seedphrase</Typography>
      </PromiseInvokeButton>
    </MenuItem>
  );
}
