import RefreshIcon from "@mui/icons-material/Refresh";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { checkBitcoinBalance } from "renderer/rpc";
import { useIsSyncingBitcoin } from "store/hooks";
import { isContextWithBitcoinWallet } from "models/tauriModelExt";

export default function WalletRefreshButton() {
  const isSyncing = useIsSyncingBitcoin();

  return (
    <PromiseInvokeButton
      endIcon={<RefreshIcon />}
      isIconButton
      isLoadingOverride={isSyncing}
      onInvoke={() => checkBitcoinBalance()}
      displayErrorSnackbar
      size="small"
      contextRequirement={isContextWithBitcoinWallet}
    />
  );
}
