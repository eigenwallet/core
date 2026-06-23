import {
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Paper,
  Tooltip,
  Typography,
} from "@mui/material";
import CircleIcon from "@mui/icons-material/Circle";
import { useState } from "react";
import Jdenticon from "renderer/components/other/Jdenticon";
import {
  BidQuote,
  QuoteWithAddress,
  RefundPolicyWire,
} from "models/tauriModel";
import {
  MoneroSatsExchangeRate,
  MoneroSatsMarkup,
  SatsAmount,
} from "renderer/components/other/Units";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { useResolveSelectMakerApproval } from "./useResolveSelectMakerApproval";
import WarningIcon from "@mui/icons-material/Warning";
import FavoriteIcon from "@mui/icons-material/Favorite";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import {
  isMakerVersionLatest,
  isMakerVersionOld,
  isMakerVersionTooOld,
} from "utils/multiAddrUtils";
import { useGuiVersion } from "utils/useGuiVersion";
import { RefundPolicy } from "store/features/settingsSlice";
import { useAppSelector } from "store/hooks";
import { BobStateName } from "models/tauriModelExt";
import { getPriorityMaker } from "utils/priorityMakers";

const FULL_WARNING_ANTI_SPAM_DEPOSIT_RATIO = 0.1;

function getRefundPercentage(policy: RefundPolicyWire): number {
  if (policy.type === "FullRefund") {
    return 100;
  }
  return policy.content.anti_spam_deposit_ratio * 100;
}

export default function MakerOfferItem({
  quoteWithAddress,
  requestId,
  showAsPriority = true,
}: {
  requestId?: string;
  quoteWithAddress: QuoteWithAddress;
  /** When false, render as a regular card even if this peer is a priority maker. */
  showAsPriority?: boolean;
}) {
  const { multiaddr, peer_id, quote, version } = quoteWithAddress;
  const isOutOfLiquidity = quote.max_quantity == 0;
  const isTooOld = isMakerVersionTooOld(version);
  const priorityMaker = showAsPriority ? getPriorityMaker(peer_id) : undefined;
  const resolveSelectMakerApproval = useResolveSelectMakerApproval();

  return (
    <Paper
      variant="outlined"
      sx={(theme) => ({
        position: "relative",
        display: "flex",
        flexDirection: "column",
        borderRadius: 2,
        padding: 2,
        width: "100%",
        ...(priorityMaker && {
          borderColor: theme.palette.primary.main,
          animation: "priorityMakerGlow 2.5s ease-in-out infinite",
          "@keyframes priorityMakerGlow": {
            "0%, 100%": {
              boxShadow: `0 0 4px ${theme.palette.primary.main}55, 0 0 8px ${theme.palette.primary.main}22`,
            },
            "50%": {
              boxShadow: `0 0 8px ${theme.palette.primary.main}aa, 0 0 16px ${theme.palette.primary.main}44`,
            },
          },
        }),
      })}
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
          {priorityMaker ? (
            <Box
              component="img"
              src={priorityMaker.avatar}
              sx={{
                width: 40,
                height: 40,
                borderRadius: "50%",
                objectFit: "cover",
                flexShrink: 0,
              }}
            />
          ) : (
            <Jdenticon value={peer_id} size={40} />
          )}
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: 0.5,
              minWidth: 0,
              flex: 1,
            }}
          >
            {priorityMaker ? (
              <Typography variant="body1" noWrap>
                <Box component="span" sx={{ color: "text.secondary" }}>
                  {peer_id}
                </Box>
              </Typography>
            ) : (
              <Typography variant="body1" color="text.secondary" noWrap>
                {peer_id}
              </Typography>
            )}
            <Typography variant="body2" color="text.secondary" noWrap>
              {multiaddr}
            </Typography>
          </Box>
        </Box>
        <PromiseInvokeButton
          variant="contained"
          onInvoke={() => resolveSelectMakerApproval(peer_id, requestId)}
          displayErrorSnackbar
          disabled={!requestId}
          tooltipTitle={
            requestId == null
              ? "You don't have enough Bitcoin to swap with this maker"
              : null
          }
          sx={
            priorityMaker
              ? {
                  transition: "transform 200ms ease",
                  "&:hover": { transform: "scale(1.04)" },
                }
              : undefined
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
        {priorityMaker && <CommunitySupporterChip />}
        {AntiSpamDepositChip(quote)}
        {ReputationChip(peer_id)}
        {version !== undefined && <VersionChip version={version} />}
        {version !== undefined && priorityMaker && (
          <LatestVersionChip version={version} />
        )}
      </Box>

      {(isOutOfLiquidity || isTooOld) && (
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
            backdropFilter: "blur(2px)",
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
              textShadow: (theme) =>
                `0 0 8px ${theme.palette.background.paper}`,
            }}
          >
            {isTooOld
              ? "Maker version incompatible (too old)"
              : "Maker has no available funds"}
          </Typography>
        </Box>
      )}
    </Paper>
  );
}

function AntiSpamDepositChip(quote: BidQuote) {
  const full_refund: boolean =
    quote.refund_policy.type === "FullRefund"
      ? true
      : quote.refund_policy.content.anti_spam_deposit_ratio === 0;
  // Rounded to 0.001 precision
  const earnest_deposit_ratio =
    Math.round(
      (quote.refund_policy.type === "FullRefund"
        ? 0
        : quote.refund_policy.content?.anti_spam_deposit_ratio) * 1000,
    ) / 1000;
  const guaranteed_refund_percentage = (1 - earnest_deposit_ratio) * 100;
  const normalized_warning_intensity = Math.min(
    earnest_deposit_ratio / FULL_WARNING_ANTI_SPAM_DEPOSIT_RATIO,
    1,
  );
  const warning_intensity = Math.sqrt(normalized_warning_intensity);

  const tooltip_text = full_refund
    ? "100% refund cryptographically guaranteed."
    : `${guaranteed_refund_percentage}% refund cryptographically guaranteed. During refunds maker may withhold the remaining ${earnest_deposit_ratio * 100}% to deter spamming. Does not apply to successful swaps`;
  const text = `${guaranteed_refund_percentage}% refund guaranteed`;

  return (
    <Tooltip title={tooltip_text} arrow>
      <Chip
        label={text}
        size="small"
        variant="outlined"
        clickable
        component="a"
        href="https://docs.eigenwallet.org/advanced/anti_spam_deposit"
        target="_blank"
        rel="noopener noreferrer"
        sx={(theme) => {
          const successMain = (theme.vars || theme).palette.success.main;
          const warningMain = (theme.vars || theme).palette.warning.main;
          const chipColor = `color-mix(in srgb, ${successMain} ${(1 - warning_intensity) * 100}%, ${warningMain} ${warning_intensity * 100}%)`;

          return {
            backgroundColor: `color-mix(in srgb, ${chipColor} ${12 + warning_intensity * 14}%, ${theme.palette.background.paper})`,
            borderColor: `color-mix(in srgb, ${chipColor} ${35 + warning_intensity * 20}%, ${theme.palette.divider})`,
            color: chipColor,
          };
        }}
      />
    </Tooltip>
  );
}

function ReputationChip(peer_id: string) {
  const allSwaps = useAppSelector((state) => state.rpc.state.swapInfos);
  if (!allSwaps) {
    return <></>;
  }
  const swapsWithThisPeer = Object.values(allSwaps).filter(
    (swap) => swap.seller.peer_id == peer_id,
  );

  const successfulSwaps = swapsWithThisPeer.filter(
    (swap) => swap.state_name === BobStateName.XmrRedeemed,
  ).length;
  // TODO: don't hardcode this check (was swap refunded/punished?) here, put into tauriModelExt or other place
  const refundedSwaps = swapsWithThisPeer.filter((swap) =>
    [
      BobStateName.BtcRefunded,
      BobStateName.BtcEarlyRefunded,
      BobStateName.BtcMercyConfirmed,
    ].includes(swap.state_name),
  ).length;
  const failedSwaps = swapsWithThisPeer.filter((swap) =>
    [BobStateName.BtcPunished, BobStateName.BtcWithheld].includes(
      swap.state_name,
    ),
  ).length;

  return (
    <Chip
      size="small"
      label={
        <Box display="flex" style={{ gap: "0.5rem" }}>
          <Tooltip
            title={`You've made ${successfulSwaps} successful swaps with this maker.`}
          >
            <Box color="success.main">{successfulSwaps} successes</Box>
          </Tooltip>
          <Divider orientation="vertical" flexItem />
          <Tooltip
            title={`${refundedSwaps} of your swaps with this maker needed to be refunded.`}
          >
            <Box color="warning.main">{refundedSwaps} refunds</Box>
          </Tooltip>
          <Divider orientation="vertical" flexItem />
          <Tooltip
            title={`The maker has acted uncooperatively in ${failedSwaps} swaps. This means withholding the anti-spam deposit or punishing you.`}
          >
            <Box color="error.main">{failedSwaps} bad</Box>
          </Tooltip>
        </Box>
      }
    />
  );
}

function CommunitySupporterChip() {
  const [open, setOpen] = useState(false);

  return (
    <>
      <Tooltip
        title="This maker actively supports the eigenwallet community"
        arrow
      >
        <Chip
          size="small"
          icon={<FavoriteIcon sx={{ fontSize: "1rem" }} />}
          label="Community Supporter"
          onClick={() => setOpen(true)}
          sx={(theme) => ({
            backgroundColor: `color-mix(in srgb, ${theme.palette.primary.main} 18%, ${theme.palette.background.paper})`,
            borderColor: `color-mix(in srgb, ${theme.palette.primary.main} 45%, ${theme.palette.divider})`,
            color: theme.palette.primary.main,
            "& .MuiChip-icon": { color: theme.palette.primary.main },
          })}
          variant="outlined"
        />
      </Tooltip>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <FavoriteIcon color="primary" />
          Community Supporter
        </DialogTitle>
        <DialogContent>
          <DialogContentText component="div">
            <List dense>
              <ListItem sx={{ pl: 0 }}>
                <ListItemIcon sx={{ minWidth: "30px" }}>
                  <CircleIcon sx={{ fontSize: "8px" }} />
                </ListItemIcon>
                <ListItemText primary="This maker supports the development of the project" />
              </ListItem>
              <ListItem sx={{ pl: 0 }}>
                <ListItemIcon sx={{ minWidth: "30px" }}>
                  <CircleIcon sx={{ fontSize: "8px" }} />
                </ListItemIcon>
                <ListItemText primary="Swapping with this maker directly supports the project" />
              </ListItem>
              <ListItem sx={{ pl: 0 }}>
                <ListItemIcon sx={{ minWidth: "30px" }}>
                  <CircleIcon sx={{ fontSize: "8px" }} />
                </ListItemIcon>
                <ListItemText primary="You get the same security — they cannot rug you" />
              </ListItem>
            </List>
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)} color="primary">
            Got it
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

function LatestVersionChip({ version }: { version: string }) {
  const guiVersion = useGuiVersion();

  if (!isMakerVersionLatest(version, guiVersion)) return null;

  return (
    <Tooltip title="Running the latest available version" arrow>
      <Chip
        color="success"
        label={
          <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <CheckCircleIcon sx={{ fontSize: "1rem" }} />
            <Typography variant="body2">Latest version</Typography>
          </Box>
        }
        size="small"
      />
    </Tooltip>
  );
}

function VersionChip({ version }: { version: string }) {
  if (isMakerVersionTooOld(version)) {
    return (
      <Tooltip title="Incompatible software — will not work" arrow>
        <Chip
          color="error"
          label={
            <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
              <WarningIcon sx={{ fontSize: "1rem" }} />
              <Typography variant="body2">v{version}</Typography>
            </Box>
          }
          size="small"
        />
      </Tooltip>
    );
  }

  if (isMakerVersionOld(version)) {
    return (
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
    );
  }

  return (
    <Tooltip title="Up to date" arrow>
      <Chip label={`v${version}`} size="small" />
    </Tooltip>
  );
}
