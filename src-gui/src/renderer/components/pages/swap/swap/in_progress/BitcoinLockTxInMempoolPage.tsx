import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { formatConfirmations } from "utils/formatUtils";
import BitcoinTransactionInfoBox from "renderer/components/pages/swap/swap/components/BitcoinTransactionInfoBox";
import { Box, DialogContentText } from "@mui/material";

export default function BitcoinLockTxInMempoolPage({
  btc_lock_confirmations,
  btc_lock_txid,
}: TauriSwapProgressEventContent<"BtcLockTxInMempool">) {
  function description() {
    if (btc_lock_confirmations != null && btc_lock_confirmations > 0) {
      return "Bitcoin have been locked and confirmed. Waiting for the other party to lock their Monero.";
    }

    return "We have locked our Bitcoin. We are waiting for the transaction to be confirmed.";
  }

  return (
    <>
      <DialogContentText>{description()}</DialogContentText>
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "1rem",
        }}
      >
        <BitcoinTransactionInfoBox
          title="Bitcoin Lock Transaction"
          txId={btc_lock_txid}
          loading
          additionalContent={<>{formatConfirmations(btc_lock_confirmations)}</>}
        />
      </Box>
    </>
  );
}
