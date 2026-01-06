/**
 * Pages for the partial refund path of the swap.
 *
 * This path is taken when Alice only signs the partial refund transaction
 * (not the full refund). The flow is:
 *
 * 1. BtcPartialRefundPublished - TxPartialRefund is published
 * 2. BtcPartiallyRefunded - TxPartialRefund is confirmed
 * 3. Either:
 *    a. BtcAmnestyPublished -> BtcAmnestyReceived (Bob claims amnesty via TxRefundAmnesty)
 *    b. BtcRefundBurnPublished -> BtcRefundBurnt (Alice burns amnesty via TxRefundBurn)
 *       -> optionally BtcFinalAmnestyPublished -> BtcFinalAmnestyConfirmed (Alice grants final amnesty)
 */

import { Alert, Box, Button, DialogContentText, Typography } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useActiveSwapInfo } from "store/hooks";
import FeedbackInfoBox from "renderer/components/pages/help/FeedbackInfoBox";
import BitcoinTransactionInfoBox from "renderer/components/pages/swap/swap/components/BitcoinTransactionInfoBox";
import DiscordIcon from "renderer/components/icons/DiscordIcon";
import MatrixIcon from "renderer/components/icons/MatrixIcon";

export function BitcoinPartialRefundPublished({
  btc_partial_refund_txid,
  btc_lock_amount,
  btc_amnesty_amount,
}: TauriSwapProgressEventContent<"BtcPartialRefundPublished">) {
  return (
    <PartialRefundPage
      txid={btc_partial_refund_txid}
      confirmed={false}
      btcLockAmount={btc_lock_amount}
      btcAmnestyAmount={btc_amnesty_amount}
    />
  );
}

export function BitcoinPartiallyRefunded({
  btc_partial_refund_txid,
  btc_lock_amount,
  btc_amnesty_amount,
}: TauriSwapProgressEventContent<"BtcPartiallyRefunded">) {
  return (
    <PartialRefundPage
      txid={btc_partial_refund_txid}
      confirmed={true}
      btcLockAmount={btc_lock_amount}
      btcAmnestyAmount={btc_amnesty_amount}
    />
  );
}

function PartialRefundPage({
  txid,
  confirmed,
  btcLockAmount,
  btcAmnestyAmount,
}: {
  txid: string;
  confirmed: boolean;
  btcLockAmount: number;
  btcAmnestyAmount: number;
}) {
  const swap = useActiveSwapInfo();

  const guaranteedPercent = Math.round(((btcLockAmount - btcAmnestyAmount) / btcLockAmount) * 100);
  const atRiskPercent = Math.round((btcAmnestyAmount / btcLockAmount) * 100);

  const mainMessage = confirmed
    ? `Refunded the first ${guaranteedPercent}% of your Bitcoin. The maker has a short time window to revoke the remaining ${atRiskPercent}%. Unless they do that we will claim it shortly.`
    : `Refunding the first ${guaranteedPercent}% of your Bitcoin. The maker has a short time window to revoke the remaining ${atRiskPercent}%. Unless they do that we will claim it shortly.`;

  const additionalContent = swap ? (
    <>
      {!confirmed && "Waiting for transaction to be confirmed..."}
      {!confirmed && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

  return (
    <>
      <DialogContentText sx={{ mb: 2 }}>{mainMessage}</DialogContentText>
      <Alert severity="info" sx={{ mb: 2 }}>
        <Typography variant="body2">
          <strong>Patience:</strong> We are first claiming the guaranteed <strong>{guaranteedPercent}%</strong> of the Bitcoin refund.
          It is <strong>not guaranteed</strong> that we can claim the remaining <strong>{atRiskPercent}%</strong>.
          We will be able to claim the remaining Bitcoin shortly unless the market maker decides to revoke it.
        </Typography>
      </Alert>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <BitcoinTransactionInfoBox
          title="Partial Refund Transaction"
          txId={txid}
          loading={!confirmed}
          additionalContent={additionalContent}
        />
      </Box>
    </>
  );
}

// Amnesty pages - We're claiming the remaining Bitcoin ourselves (good outcome)

export function BitcoinAmnestyPublished({
  btc_amnesty_txid,
}: TauriSwapProgressEventContent<"BtcAmnestyPublished">) {
  return (
    <AmnestyPage txid={btc_amnesty_txid} confirmed={false} />
  );
}

export function BitcoinAmnestyReceived({
  btc_amnesty_txid,
}: TauriSwapProgressEventContent<"BtcAmnestyReceived">) {
  return (
    <AmnestyPage txid={btc_amnesty_txid} confirmed={true} />
  );
}

function AmnestyPage({
  txid,
  confirmed,
}: {
  txid: string;
  confirmed: boolean;
}) {
  const swap = useActiveSwapInfo();

  const mainMessage = confirmed
    ? "All your Bitcoin has been refunded. The swap is complete."
    : "The remaining Bitcoin is being released to you. Waiting for confirmation.";

  const additionalContent = swap ? (
    <>
      {!confirmed && "Waiting for transaction to be confirmed..."}
      {!confirmed && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

  return (
    <>
      <DialogContentText sx={{ mb: 2 }}>{mainMessage}</DialogContentText>
      <Alert severity="success" sx={{ mb: 2 }}>
        <Typography variant="body2">
          <strong>{confirmed ? "Complete:" : "Almost there:"}</strong> The
          remaining Bitcoin from your partial refund{" "}
          {confirmed ? "has been" : "is being"} released to you.
        </Typography>
      </Alert>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <BitcoinTransactionInfoBox
          title="Remaining Refund Transaction"
          txId={txid}
          loading={!confirmed}
          additionalContent={additionalContent}
        />
        <FeedbackInfoBox />
      </Box>
    </>
  );
}

// Refund Burn pages - The maker actively burned the remaining Bitcoin (bad outcome)
// Note: By default, the user would have received the remaining Bitcoin after a timelock.
// If we're in this state, it means the maker actively published TxBurn to revoke it.

export function BitcoinRefundBurnPublished({
  btc_refund_burn_txid,
  btc_lock_amount,
  btc_amnesty_amount,
}: TauriSwapProgressEventContent<"BtcRefundBurnPublished">) {
  return (
    <RefundBurnPage
      txid={btc_refund_burn_txid}
      confirmed={false}
      btcLockAmount={btc_lock_amount}
      btcAmnestyAmount={btc_amnesty_amount}
    />
  );
}

export function BitcoinRefundBurnt({
  btc_refund_burn_txid,
  btc_lock_amount,
  btc_amnesty_amount,
}: TauriSwapProgressEventContent<"BtcRefundBurnt">) {
  return (
    <RefundBurnPage
      txid={btc_refund_burn_txid}
      confirmed={true}
      btcLockAmount={btc_lock_amount}
      btcAmnestyAmount={btc_amnesty_amount}
    />
  );
}

function RefundBurnPage({
  txid,
  confirmed,
  btcLockAmount,
  btcAmnestyAmount,
}: {
  txid: string;
  confirmed: boolean;
  btcLockAmount: number;
  btcAmnestyAmount: number;
}) {
  const atRiskPercent = Math.round((btcAmnestyAmount / btcLockAmount) * 100);

  const mainMessage = confirmed
    ? "The market maker has revoked your remaining Bitcoin refund."
    : "The market maker is revoking your remaining Bitcoin refund.";

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <DialogContentText>{mainMessage}</DialogContentText>
      <Alert severity="error">
        <Typography variant="body2">
          <strong>Refund revoked:</strong> The market maker has revoked the remaining <strong>{atRiskPercent}%</strong> of your Bitcoin refund.
          This portion is now lost and we cannot recover it on our own.
        </Typography>
      </Alert>
      <Alert severity="info">
        <Typography variant="body2">
          <strong>Why did this happen?</strong> Aborting a swap incurs significant costs on makers.
          To prevent spam attacks, they can revoke a previously agreed upon part of the refund.
          The maker has exercised this option because they think you are spamming them.
        </Typography>
      </Alert>
      <Alert severity="info">
        <Typography variant="body2">
          <strong>You can appeal.</strong> If you did not mean to spam the market maker, contact them through our official
          community. The maker can still help you recover the remaining Bitcoin.
        </Typography>
        <br />
        <Box sx={{ display: "flex", justifyContent: "center", gap: 2 }}>
          <Button
            variant="outlined"
            startIcon={<MatrixIcon />}
            href="https://eigenwallet.org/matrix"
            target="_blank"
          >
            Matrix
          </Button>
          <Button
            variant="outlined"
            startIcon={<DiscordIcon />}
            href="https://eigenwallet.org/discord"
            target="_blank"
          >
            Discord
          </Button>
        </Box>
      </Alert>
      <BitcoinTransactionInfoBox
        title="Burn Transaction"
        txId={txid}
        loading={!confirmed}
        additionalContent={
          !confirmed ? "Waiting for transaction to be confirmed..." : null
        }
      />
    </Box>
  );
}

// Final Amnesty pages - The maker granted final amnesty after the user appealed

export function BitcoinFinalAmnestyPublished({
  btc_final_amnesty_txid,
}: TauriSwapProgressEventContent<"BtcFinalAmnestyPublished">) {
  return <FinalAmnestyPage txid={btc_final_amnesty_txid} confirmed={false} />;
}

export function BitcoinFinalAmnestyConfirmed({
  btc_final_amnesty_txid,
}: TauriSwapProgressEventContent<"BtcFinalAmnestyConfirmed">) {
  return <FinalAmnestyPage txid={btc_final_amnesty_txid} confirmed={true} />;
}

function FinalAmnestyPage({
  txid,
  confirmed,
}: {
  txid: string;
  confirmed: boolean;
}) {
  const swap = useActiveSwapInfo();

  const mainMessage = confirmed
    ? "The market maker has granted you final amnesty. The remaining Bitcoin has been recovered."
    : "The market maker is granting you final amnesty. Waiting for confirmation.";

  const additionalContent = swap ? (
    <>
      {!confirmed && "Waiting for transaction to be confirmed..."}
      {!confirmed && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

  return (
    <>
      <DialogContentText sx={{ mb: 2 }}>{mainMessage}</DialogContentText>
      <Alert severity="success" sx={{ mb: 2 }}>
        <Typography variant="body2">
          <strong>Appeal successful:</strong> The market maker has decided to
          release the remaining Bitcoin to you. All your Bitcoin has now been
          fully refunded.
        </Typography>
      </Alert>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <BitcoinTransactionInfoBox
          title="Final Amnesty Transaction"
          txId={txid}
          loading={!confirmed}
          additionalContent={additionalContent}
        />
      </Box>
    </>
  );
}
