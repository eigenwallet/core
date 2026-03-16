/**
 * Pages for the partial refund path of the swap.
 *
 * This path is taken when Alice only signs the partial refund transaction
 * (not the full refund). The flow is:
 *
 * 1. BtcPartialRefundPublished - TxPartialRefund is published
 * 2. BtcPartiallyRefunded - TxPartialRefund is confirmed
 * 3. Either:
 *    a. BtcAmnestyPublished -> BtcAmnestyReceived (Bob claims amnesty via TxReclaim)
 *    b. BtcWithholdPublished -> BtcWithheld (Alice withholds amnesty via TxWithhold)
 *       -> optionally BtcMercyPublished -> BtcMercyConfirmed (Alice grants mercy)
 */

import { Alert, Box, Button, DialogContentText, Link, Typography } from "@mui/material";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { useActiveSwapInfo, useAppSelector } from "store/hooks";
import FeedbackInfoBox from "renderer/components/pages/help/FeedbackInfoBox";
import BitcoinTransactionInfoBox from "renderer/components/pages/swap/swap/components/BitcoinTransactionInfoBox";
import DiscordIcon from "renderer/components/icons/DiscordIcon";
import MatrixIcon from "renderer/components/icons/MatrixIcon";
import { Book } from "@mui/icons-material";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";

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
  const blocksConfirmed = target_blocks - blocks_until_expiry;
  const atRiskPercent = Math.round((btc_amnesty_amount / btc_lock_amount) * 100);

  return (
    <>
      <DialogContentText>
        Waiting for the timelock on the anti-spam deposit ({atRiskPercent}% of your Bitcoin) to expire.
        The maker can choose to withhold it during this time.
        After the timelock expires, we will refund the remaining Bitcoin.
      </DialogContentText>
      <BitcoinTransactionInfoBox
        title="Waiting for timelock to expire"
        txId={btc_partial_refund_txid}
        loading
        additionalContent={`${blocksConfirmed}/${target_blocks} blocks`}
      />
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

  const additionalContent = swap ? (
    <>
      {!confirmed && "Waiting for transaction to be confirmed..."}
      {!confirmed && <br />}
      Refund address: {swap.btc_refund_address}
    </>
  ) : null;

  return (
    <>
      <DialogContentText>
        We are first taking the guaranteed <strong>{guaranteedPercent}%</strong> Bitcoin refund.
        After a short timelock we will be able to reclaim the anti-spam deposit (the remaining {atRiskPercent}%).
        The maker may in rare circumstances withhold the deposit.
        <br />
        <Link href={"https://docs.eigenwallet.org/advanced/anti_spam_deposit"} target="_blank">Read more</Link>
      </DialogContentText>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <BitcoinTransactionInfoBox
          title={`Partial Refund Transaction (${guaranteedPercent}% of the Bitcoin)`}
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
    : "The remaining Bitcoin (anti-spam deposit) are being released to you. Waiting for confirmation.";

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

// Withhold pages - The maker actively withheld the remaining Bitcoin (bad outcome)
// Note: By default, the user would have received the remaining Bitcoin after a timelock.
// If we're in this state, it means the maker actively published TxWithhold to revoke it.

export function BitcoinWithholdPublished({
  btc_withhold_txid,
  btc_lock_amount,
  btc_amnesty_amount,
}: TauriSwapProgressEventContent<"BtcWithholdPublished">) {
  return (
    <WithholdPage
      txid={btc_withhold_txid}
      confirmed={false}
      btcLockAmount={btc_lock_amount}
      btcAmnestyAmount={btc_amnesty_amount}
    />
  );
}

export function BitcoinWithheld({
  btc_withhold_txid,
  btc_lock_amount,
  btc_amnesty_amount,
}: TauriSwapProgressEventContent<"BtcWithheld">) {
  return (
    <WithholdPage
      txid={btc_withhold_txid}
      confirmed={true}
      btcLockAmount={btc_lock_amount}
      btcAmnestyAmount={btc_amnesty_amount}
    />
  );
}

function ContactIdentifierBox({
  label,
  helperText,
  value,
}: {
  label: string;
  helperText: string;
  value: string;
}) {
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
      <Typography variant="body2" sx={{ fontWeight: 600 }}>
        {label}
      </Typography>
      <Typography variant="caption" color="text.secondary">
        {helperText}
      </Typography>
      <ActionableMonospaceTextBox
        content={value}
        displayCopyIcon={true}
        enableQrCode={false}
      />
    </Box>
  );
}

function WithholdPage({
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
  const swapInfo = useActiveSwapInfo();
  const isMock = useAppSelector((s) => s.swap._mockOnlyDisableTauriCallsOnSwapProgress);
  const swapId = swapInfo?.swap_id ?? (isMock ? "a1b2c3d4-e5f6-7890-abcd-ef1234567890" : null);
  const peerId = swapInfo?.seller.peer_id ?? (isMock ? "12D3KooWF1rGmFnqJhNrHhEMPVbMM3eRnuf3XPG3JcvedAMdSHkj" : null);
  const atRiskPercent = Math.round((btcAmnestyAmount / btcLockAmount) * 100);

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <DialogContentText>
        <p>
          The maker is withholding the anti-spam deposit (remaining {atRiskPercent}% of the Bitcoin)
          because they think you are spamming them.
        </p>
        <p>
          They can still release the deposit if you convince them otherwise.
          Share the swap ID below when you contact them.
          The maker Peer ID helps confirm you are speaking to the correct market maker.
        </p>
        <p>
          You can reach out to them on our community servers.
        </p>
        <p>
          <i>Beware scammers.
            Never reveal your private key or seedphrase to anyone.</i>
        </p>
        <Box sx={{ display: "flex", justifyContent: "center", gap: 2 }}>
          <Button
            variant="outlined"
            startIcon={<Book />}
            href="https://docs.eigenwallet.org/advanced/anti_spam_deposit"
            target="_blank"
          >
            Docs + FAQ
          </Button>
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
      </DialogContentText>
      <BitcoinTransactionInfoBox
        title="Withhold Transaction"
        txId={txid}
        loading={!confirmed}
        additionalContent={
          !confirmed ? "Waiting for transaction to be confirmed..." : null
        }
      />
      {(swapId != null || peerId != null) && (
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          {swapId != null && (
            <ContactIdentifierBox
              label="Swap ID"
              helperText="So the maker can find your swap."
              value={swapId}
            />
          )}
          {peerId != null && (
            <ContactIdentifierBox
              label="Maker Peer ID"
              helperText="The maker you are swapping with."
              value={peerId}
            />
          )}
        </Box>
      )}
    </Box >
  );
}

// Mercy pages - The maker granted mercy after the user appealed

export function BitcoinMercyPublished({
  btc_mercy_txid,
}: TauriSwapProgressEventContent<"BtcMercyPublished">) {
  return <MercyPage txid={btc_mercy_txid} confirmed={false} />;
}

export function BitcoinMercyConfirmed({
  btc_mercy_txid,
}: TauriSwapProgressEventContent<"BtcMercyConfirmed">) {
  return <MercyPage txid={btc_mercy_txid} confirmed={true} />;
}

function MercyPage({
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
