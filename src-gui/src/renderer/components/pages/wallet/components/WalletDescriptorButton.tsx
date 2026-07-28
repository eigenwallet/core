import { Key as KeyIcon } from "@mui/icons-material";
import { isContextWithBitcoinWallet } from "models/tauriModelExt";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { getWalletRecovery, WalletRecoveryData } from "renderer/rpc";

export default function WalletDescriptorButton({
  onShowSeed,
}: {
  onShowSeed: (walletRecovery: WalletRecoveryData) => void;
}) {
  return (
    <PromiseInvokeButton
      isChipButton={true}
      startIcon={<KeyIcon />}
      onInvoke={getWalletRecovery}
      onSuccess={onShowSeed}
      displayErrorSnackbar={true}
      contextRequirement={isContextWithBitcoinWallet}
    >
      Show Bitcoin Seed
    </PromiseInvokeButton>
  );
}
