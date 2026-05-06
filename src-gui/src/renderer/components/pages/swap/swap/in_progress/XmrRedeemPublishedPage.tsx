import { DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import MoneroTransactionInfoBox from "../components/MoneroTransactionInfoBox";

export default function XmrRedeemPublishedPage(
  state: TauriSwapProgressEventContent<"XmrRedeemPublished">,
) {
  const xmr_redeem_txid = state.xmr_redeem_txids[0] ?? null;

  return (
    <>
      <DialogContentText>
        We have published the Monero redeem transaction. Waiting for it to be
        included in a block.
      </DialogContentText>
      <MoneroTransactionInfoBox
        title="Monero Redeem Transaction"
        txId={xmr_redeem_txid}
        additionalContent={null}
        loading
      />
    </>
  );
}
