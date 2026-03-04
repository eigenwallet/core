import { Box, Chip, Divider, Paper, Tooltip, Typography } from "@mui/material";
import Jdenticon from "renderer/components/other/Jdenticon";
import { BidQuote, QuoteWithAddress, RefundPolicyWire } from "models/tauriModel";
import {
  MoneroSatsExchangeRate,
  MoneroSatsMarkup,
  SatsAmount,
} from "renderer/components/other/Units";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { resolveApproval } from "renderer/rpc";
import { isMakerVersionOutdated } from "utils/multiAddrUtils";
import WarningIcon from "@mui/icons-material/Warning";
import { RefundPolicy } from "store/features/settingsSlice";
import { useAppSelector } from "store/hooks";
import { BobStateName } from "models/tauriModelExt";

function getRefundPercentage(policy: RefundPolicyWire): number {
  if (policy.type === "FullRefund") {
    return 100;
  }
  return policy.content.anti_spam_deposit_ratio * 100;
}

export default function MakerOfferItem({
  quoteWithAddress,
  requestId,
}: {
  requestId?: string;
  quoteWithAddress: QuoteWithAddress;
}) {
  const { multiaddr, peer_id, quote, version } = quoteWithAddress;
  const isOutOfLiquidity = quote.max_quantity == 0;


  return (
    <Paper
      variant="outlined"
      sx={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        borderRadius: 2,
        padding: 2,
        width: "100%",
      }}
    >
      {/* Top section: Avatar, peer info, and select button */}
      <Box
        sx={{
          display: "flex",
          flexDirection: { xs: "column", sm: "row" },
          gap: 2,
          justifyContent: "space-between",
          alignItems: { xs: "stretch", sm: "center" },
        }}
      >
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            alignItems: "center",
            gap: 2,
            flex: 1,
            minWidth: 0,
          }}
        >
          <Jdenticon value={peer_id} size={40} />
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: 0.5,
              minWidth: 0,
              flex: 1,
            }}
          >
            <Typography variant="body1" noWrap>
              {multiaddr}
            </Typography>
            <Typography variant="body2" color="text.secondary" noWrap>
              {peer_id}
            </Typography>
          </Box>
        </Box>
        <PromiseInvokeButton
          variant="contained"
          onInvoke={() => {
            if (!requestId) {
              throw new Error("Request ID is required");
            }
            return resolveApproval(requestId, true as unknown as object);
          }}
          displayErrorSnackbar
          disabled={!requestId}
          tooltipTitle={
            requestId == null
              ? "You don't have enough Bitcoin to swap with this maker"
              : null
          }
        >
          Select
        </PromiseInvokeButton>
      </Box>

      {/* Horizontal divider */}
      <Divider sx={{ my: 2 }} />

      {/* Bottom section: Chips */}
      <Box
        sx={{
          display: "flex",
          flexDirection: "row",
          gap: 1,
          flexWrap: "wrap",
        }}
      >
        <Tooltip title="Exchange rate" arrow>
          <Chip
            label={<MoneroSatsExchangeRate rate={quote.price} />}
            size="small"
          />
        </Tooltip>
        <Tooltip title="Compared to market rate" arrow>
          <Chip
            label={
              <>
                <MoneroSatsMarkup rate={quote.price} /> markup
              </>
            }
            size="small"
          />
        </Tooltip>
        <Tooltip title="Swap limits" arrow placement="top">
          <Chip
            label={
              <>
                <SatsAmount amount={quote.min_quantity} /> –{" "}
                <SatsAmount amount={quote.max_quantity} />
              </>
            }
            size="small"
          />
        </Tooltip>
        {EarnestDepositChip(quote)}
        {ReputationChip(peer_id)}
        {isMakerVersionOutdated(version) ? (
          <Tooltip title="Outdated software — may cause issues" arrow>
            <Chip
              color="warning"
              label={
                <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                  <WarningIcon sx={{ fontSize: "1rem" }} />
                  <Typography variant="body2">v{version}</Typography>
                </Box>
              }
              size="small"
            />
          </Tooltip>
        ) : (
          <Tooltip title="Up to date" arrow>
            <Chip label={`v${version}`} size="small" />
          </Tooltip>
        )}
      </Box>

      {isOutOfLiquidity && (
        <Box
          sx={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            backdropFilter: "blur(1px)",
            borderRadius: 2,
            pointerEvents: "auto",
          }}
        >
          <Typography
            variant="h6"
            sx={{
              fontWeight: "bold",
              color: "text.secondary",
              textAlign: "center",
            }}
          >
            Maker has no available funds
          </Typography>
        </Box>
      )}
    </Paper>
  );
}

function EarnestDepositChip(quote: BidQuote) {
  const full_refund: boolean = quote.refund_policy.type === "FullRefund" ? true : quote.refund_policy.content.anti_spam_deposit_ratio === 0;
  // Rounded to 0.001 precision
  const earnest_deposit_ratio = Math.round(
    (quote.refund_policy.type === "FullRefund" ? 0 : quote.refund_policy.content?.anti_spam_deposit_ratio)
    * 1000
  ) / 1000;
  const guaranteed_refund_percentage = (1 - earnest_deposit_ratio) * 100;

  const tooltip_text = full_refund ? "100% refund cryptographically guaranteed." : `${guaranteed_refund_percentage}% refund cryptographically guaranteed. During refunds maker may withhold the remaining ${earnest_deposit_ratio * 100}% to protect themselves against griefing. Does not apply to successful swaps`;
  const text = `${guaranteed_refund_percentage}% refund guaranteed`;

  // TODO: use colors better to distinguish between low deposits (1%) and high ones (20%)

  return <Tooltip
    title={tooltip_text}
    arrow
  >
    <Chip
      label={text}
      size="small"
      color={full_refund ? "success" : "warning"} />
  </Tooltip>;
}

function ReputationChip(peer_id: string) {
  const allSwaps = useAppSelector(state => state.rpc.state.swapInfos)
  if (!allSwaps) { return <></> }
  const swapsWithThisPeer = Object.values(allSwaps).filter(swap => swap.seller.peer_id == peer_id)

  const successfulSwaps = swapsWithThisPeer.filter(swap => swap.state_name === BobStateName.XmrRedeemed).length
  // TODO: don't hardcode this check (was swap refunded/punished?) here, put into tauriModelExt or other place
  const refundedSwaps = swapsWithThisPeer.filter(swap => [BobStateName.BtcRefunded, BobStateName.BtcEarlyRefunded, BobStateName.BtcMercyConfirmed].includes(swap.state_name)).length
  const failedSwaps = swapsWithThisPeer.filter(swap => [BobStateName.BtcPunished, BobStateName.BtcWithheld].includes(swap.state_name)).length

  return <Tooltip
    title={"How many swaps you made with this maker and how they turned out. \"Fail\" means you have been punished or you refunded and the maker withheld the anti-spam deposit."}
    arrow
  >
    <Chip
      size="small"
      label={
        <Box display="flex" style={{ gap: "0.5rem" }}>
          <Box color="success.main">{successfulSwaps} success</Box>
          <Divider orientation="vertical" flexItem />
          <Box color="warning.main">{refundedSwaps} refund</Box>
          <Divider orientation="vertical" flexItem />
          <Box color="error.main">{failedSwaps} fail</Box>
        </Box>
      }
    />
  </Tooltip>
}
