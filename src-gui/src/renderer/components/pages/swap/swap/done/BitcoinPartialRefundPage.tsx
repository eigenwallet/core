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

import { Alert, Box, Button, DialogContentText, LinearProgress, Typography } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import HumanizedBitcoinBlockDuration from "renderer/components/other/HumanizedBitcoinBlockDuration";
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

export function WaitingForEarnestDepositTimelockExpirationPage({
  btc_partial_refund_txid,
  btc_lock_amount,
  btc_amnesty_amount,
  target_blocks,
  blocks_until_expiry,
}: TauriSwapProgressEventContent<"WaitingForEarnestDepositTimelockExpiration">) {
  const swap = useActiveSwapInfo();

  const blocksConfirmed = target_blocks - blocks_until_expiry;
  const progressPercent = Math.round((blocksConfirmed / target_blocks) * 100);
  const atRiskPercent = Math.round((btc_amnesty_amount / btc_lock_amount) * 100);

  const additionalContent = swap ? (
    <>Refund address: {swap.btc_refund_address}</>
  ) : null;

  return (
    <>
      <DialogContentText sx={{ mb: 2 }}>
        Waiting to claim the earnest deposit ({atRiskPercent}% of your Bitcoin).
        The maker can still withhold it during this time.
      </DialogContentText>
      <Alert severity="info" sx={{ mb: 2 }}>
        <Typography variant="body2">
          <strong>Timelock progress:</strong> {blocksConfirmed} of {target_blocks} blocks confirmed ({progressPercent}%).
          {blocks_until_expiry > 0 && (
            <>
              {" "}Approximately <HumanizedBitcoinBlockDuration blocks={blocks_until_expiry} /> remaining.
            </>
          )}
        </Typography>
        <LinearProgress
          variant="determinate"
          value={progressPercent}
          sx={{ mt: 1, height: 8, borderRadius: 1 }}
        />
      </Alert>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <BitcoinTransactionInfoBox
          title="Partial Refund Transaction"
          txId={btc_partial_refund_txid}
          loading={true}
          additionalContent={additionalContent}
        />
      </Box>
    </>
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
    ? `Refunded the first ${guaranteedPercent}% of your Bitcoin. The maker has a short time window to withhold the earnest deposit of ${atRiskPercent}%. Unless they do that we will claim it shortly.`
    : `Refunding the first ${guaranteedPercent}% of your Bitcoin. The maker has a short time window to withhold the earnest deposit of ${atRiskPercent}%. Unless they do that we will claim it shortly.`;

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
          <strong>Patience:</strong> We are first refunding the guaranteed <strong>{guaranteedPercent}%</strong> of the Bitcoin refund.
          It is <strong>not guaranteed</strong> that we can claim the earnest deposit, which makes up the remaining <strong>{atRiskPercent}%</strong>.
          The maker has a short timeframe to withhold the deposit, after that we can claim it.
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
    ? "All your Bitcoin have been refunded. The swap is complete."
    : "The remaining Bitcoin (earnest deposit) are being released to you. Waiting for confirmation.";

  const additionalContent = swap ? (
    <>
      {!confirmed && "Waiting for transaction to be confirmed..."}
      {!confirmed && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

  return (
    <>
      <Alert severity="success" sx={{ mb: 2 }}>
        <Typography variant="body2">
          <strong>{confirmed ? "Complete:" : "Almost there:"}</strong>{" "}{mainMessage}
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

// Refund Burn pages - The maker actively withheld the remaining Bitcoin (bad outcome)
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

  const mainMessage = "The market maker is withholding the earnest deposit."

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <DialogContentText>{mainMessage}</DialogContentText>
      <Alert severity="error">
        <Typography variant="body2">
          <strong>Earnest deposit withheld:</strong> The market maker has choosen to withhold the remaining <strong>{atRiskPercent}%</strong> of your Bitcoin refund.

        </Typography>
      </Alert>
      <Alert severity="info">
        <Typography variant="body2" gutterBottom>
          <strong>Why did this happen?</strong> Aborting a swap incurs significant costs on makers.
          To prevent spam attacks, makers can choose to require an "earnest deposit",
          which they can withhold if the swap is aborted.
        </Typography>
        <Typography variant="body2">
          Makers do not have access to the withheld deposit.
          The maker you are swapping with has exercised their option to withhold, because they think you are spamming them.
        </Typography>
      </Alert>
      <Alert severity="info">
        <Typography variant="body2">
          <strong>You can contact the maker:</strong> If you think this was a mistake, you can contact the maker through our official
          community channels.
          The maker can still release the deposit.
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
        title="Withhold Transaction"
        txId={txid}
        loading={!confirmed}
        additionalContent={
          !confirmed ? "Waiting for transaction to be confirmed..." : null
        }
      />
    </Box >
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
    ? "The market maker has release the earnest deposit they withheld. The refund is complete."
    : "The market maker is releasing the earnest deposit they withheld. Waiting for transaction confirmation.";

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
          <strong>Mercy granted:</strong> The market maker has decided to
          release the earnest deposit, which they previously withheld. All your Bitcoin has now been
          fully refunded.
        </Typography>
      </Alert>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <BitcoinTransactionInfoBox
          title="Mercy Transaction"
          txId={txid}
          loading={!confirmed}
          additionalContent={additionalContent}
        />
      </Box>
    </>
  );
}
