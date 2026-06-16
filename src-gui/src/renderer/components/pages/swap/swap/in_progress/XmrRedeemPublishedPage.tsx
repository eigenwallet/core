import { DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SwapMoneroRecoveryButton } from "renderer/components/pages/history/table/SwapMoneroRecoveryButton";
import { useActiveSwapInfo } from "store/hooks";
import MoneroTransactionInfoBox from "../components/MoneroTransactionInfoBox";

export default function XmrRedeemPublishedPage(
  state: TauriSwapProgressEventContent<"XmrRedeemPublished">,
) {
  const xmr_redeem_txid = state.xmr_redeem_txids[0] ?? null;
  const swap = useActiveSwapInfo();

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
    </>
  );
}
