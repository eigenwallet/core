import { ListItemIcon, MenuItem, Typography } from "@mui/material";
import { Key as KeyIcon } from "@mui/icons-material";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { getWalletRecovery, WalletRecoveryData } from "renderer/rpc";
import { isContextWithMoneroWallet } from "models/tauriModelExt";

interface SeedPhraseButtonProps {
  onSeedPhraseSuccess: (response: WalletRecoveryData) => void;
}

export default function SeedPhraseButton({
  onSeedPhraseSuccess,
}: SeedPhraseButtonProps) {
  return (
    <PromiseInvokeButton
      onInvoke={getWalletRecovery}
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
      <Typography>Wallet Recovery</Typography>
    </PromiseInvokeButton>
  );
}
