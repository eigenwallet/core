import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useSwapInfo } from "store/hooks";
import FeedbackInfoBox from "renderer/components/pages/help/FeedbackInfoBox";
import BitcoinTransactionInfoBox from "renderer/components/pages/swap/swap/components/BitcoinTransactionInfoBox";

export function BitcoinRefundPublishedPage({
  swapId,
  btc_refund_txid,
}: { swapId: string } & TauriSwapProgressEventContent<"BtcRefundPublished">) {
  return (
    <MultiBitcoinRefundedPage
      swapId={swapId}
      btc_refund_txid={btc_refund_txid}
      btc_refund_finalized={false}
    />
  );
}

export function BitcoinEarlyRefundPublishedPage({
  swapId,
  btc_early_refund_txid,
}: {
  swapId: string;
} & TauriSwapProgressEventContent<"BtcEarlyRefundPublished">) {
  return (
    <MultiBitcoinRefundedPage
      swapId={swapId}
      btc_refund_txid={btc_early_refund_txid}
      btc_refund_finalized={false}
    />
  );
}

export function BitcoinRefundedPage({
  swapId,
  btc_refund_txid,
}: { swapId: string } & TauriSwapProgressEventContent<"BtcRefunded">) {
  return (
    <MultiBitcoinRefundedPage
      swapId={swapId}
      btc_refund_txid={btc_refund_txid}
      btc_refund_finalized={true}
    />
  );
}

export function BitcoinEarlyRefundedPage({
  swapId,
  btc_early_refund_txid,
}: { swapId: string } & TauriSwapProgressEventContent<"BtcEarlyRefunded">) {
  return (
    <MultiBitcoinRefundedPage
      swapId={swapId}
      btc_refund_txid={btc_early_refund_txid}
      btc_refund_finalized={true}
    />
  );
}

function MultiBitcoinRefundedPage({
  swapId,
  btc_refund_txid,
  btc_refund_finalized,
}: {
  swapId: string;
  btc_refund_txid: string;
  btc_refund_finalized: boolean;
}) {
  const swap = useSwapInfo(swapId);
  const additionalContent = swap ? (
    <>
      {!btc_refund_finalized &&
        "Waiting for refund transaction to be confirmed"}
      {!btc_refund_finalized && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

  return (
    <>
      <DialogContentText>
        Unfortunately, the swap was not successful. However, rest assured that
        all your Bitcoin has been refunded to the specified address.{" "}
        {btc_refund_finalized &&
          "The swap process is now complete, and you are free to exit the application."}
      </DialogContentText>
      <Box
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "0.5rem",
        }}
      >
        <BitcoinTransactionInfoBox
          title="Bitcoin Refund Transaction"
          txId={btc_refund_txid}
          loading={!btc_refund_finalized}
          additionalContent={additionalContent}
        />
        <FeedbackInfoBox />
      </Box>
    </>
  );
}
