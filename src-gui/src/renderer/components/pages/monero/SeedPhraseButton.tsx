import { ListItemIcon, MenuItem, Typography } from "@mui/material";
import { Key as KeyIcon } from "@mui/icons-material";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { getMoneroSeedAndRestoreHeight } from "renderer/rpc";
import {
  GetMoneroSeedResponse,
  GetRestoreHeightResponse,
} from "models/tauriModel";
import { isContextWithMoneroWallet } from "models/tauriModelExt";

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
  return (
    <PromiseInvokeButton
      onInvoke={getMoneroSeedAndRestoreHeight}
      onSuccess={onSeedPhraseSuccess}
      displayErrorSnackbar={true}
      contextRequirement={isContextWithMoneroWallet}
      component={MenuItem}
      disableRipple={false}
      sx={{
        textTransform: "none",
        width: "100%",
        borderRadius: "0px",
      }}
      color="inherit"
    >
      <ListItemIcon>
        <KeyIcon />
      </ListItemIcon>
      <Typography>Seedphrase</Typography>
    </PromiseInvokeButton>
  );
}
