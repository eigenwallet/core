import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SwapMoneroRecoveryButton } from "renderer/components/pages/history/table/SwapMoneroRecoveryButton";
import { useActiveSwapInfo } from "store/hooks";
import MoneroTransactionInfoBox from "../components/MoneroTransactionInfoBox";

export default function WaitingForXmrConfirmationsBeforeRedeemPage({
  xmr_lock_txid,
  xmr_lock_tx_confirmations,
  xmr_lock_tx_target_confirmations,
}: TauriSwapProgressEventContent<"WaitingForXmrConfirmationsBeforeRedeem">) {
  const swap = useActiveSwapInfo();

  return (
    <Box>
      <DialogContentText>
        We are waiting for the Monero lock transaction to receive enough
        confirmations before we can sweep them to your address.
      </DialogContentText>

      <MoneroTransactionInfoBox
        title="Monero Lock Transaction"
        txId={xmr_lock_txid}
        additionalContent={`Confirmations: ${xmr_lock_tx_confirmations}/${xmr_lock_tx_target_confirmations}`}
        loading
      />
      {swap && (
        <SwapMoneroRecoveryButton
          swap={swap}
          variant="text"
          size="small"
          sx={(theme) => ({ color: theme.palette.text.secondary })}
        >
          Redeem manually
        </SwapMoneroRecoveryButton>
      )}
    </Box>
  );
}
