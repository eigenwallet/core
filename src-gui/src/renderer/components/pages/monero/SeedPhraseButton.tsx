import { ListItemIcon, MenuItem, Typography } from "@mui/material";
import { Key as KeyIcon } from "@mui/icons-material";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { getMoneroSeedAndRestoreHeight } from "renderer/rpc";
import {
  GetMoneroSeedResponse,
  GetRestoreHeightResponse,
} from "models/tauriModel";

interface SeedPhraseButtonProps {
  onMenuClose: () => void;
  onSeedPhraseSuccess: (
    response: [GetMoneroSeedResponse, GetRestoreHeightResponse],
  ) => void;
}

export default function SeedPhraseButton({
  onMenuClose,
  onSeedPhraseSuccess,
}: SeedPhraseButtonProps) {
  const handleSeedPhraseSuccess = (
    response: [GetMoneroSeedResponse, GetRestoreHeightResponse],
  ) => {
    onSeedPhraseSuccess(response);
    onMenuClose();
  };

  return (
    <MenuItem component="div">
      <PromiseInvokeButton
        onInvoke={getMoneroSeedAndRestoreHeight}
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
