import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { formatConfirmations } from "utils/formatUtils";
import MoneroTransactionInfoBox from "../components/MoneroTransactionInfoBox";

export default function XmrLockTxInMempoolPage({
  xmr_lock_tx_confirmations,
  xmr_lock_txid,
  xmr_lock_tx_target_confirmations,
}: TauriSwapProgressEventContent<"XmrLockTxInMempool">) {
  return (
    <>
      <DialogContentText>
        They have locked the Monero. The swap will proceed once the transaction
        has been confirmed.
      </DialogContentText>

      <MoneroTransactionInfoBox
        title="Monero Lock Transaction"
        txId={xmr_lock_txid}
        additionalContent={formatConfirmations(
          xmr_lock_tx_confirmations,
          xmr_lock_tx_target_confirmations,
        )}
        loading
      />
    </>
  );
}
