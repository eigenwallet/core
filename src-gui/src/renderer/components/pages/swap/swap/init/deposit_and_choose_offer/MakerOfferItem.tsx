import { Box, Button, Chip, Paper, Tooltip, Typography } from "@mui/material";
import Avatar from "boring-avatars";
import { QuoteWithAddress } from "models/tauriModel";
import {
  MoneroSatsExchangeRate,
  SatsAmount,
} from "renderer/components/other/Units";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { resolveApproval } from "renderer/rpc";
import { isMakerVersionOutdated } from "utils/multiAddrUtils";
import { getMarkup, satsToBtc } from "utils/conversionUtils";
import { useAppSelector } from "store/hooks";
import WarningIcon from "@mui/icons-material/Warning";

export default function MakerOfferItem({
  quoteWithAddress,
  requestId,
  noButton = false,
}: {
  requestId?: string;
  quoteWithAddress: QuoteWithAddress;
  noButton?: boolean;
}) {
  const { multiaddr, peer_id, quote, version } = quoteWithAddress;
  const marketExchangeRate = useAppSelector((s) => s.rates?.xmrBtcRate);

  // Calculate markup if market rate is available
  const markup = marketExchangeRate
    ? getMarkup(satsToBtc(quote.price), marketExchangeRate)
    : null;

  return (
    <Paper
      variant="outlined"
      sx={{
        display: "flex",
        flexDirection: { xs: "column", sm: "row" },
        gap: 2,
        borderRadius: 2,
        padding: 2,
        width: "100%",
        justifyContent: "space-between",
        alignItems: { xs: "stretch", sm: "center" },
        minWidth: 0, // Allow shrinking
      }}
    >
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          flex: 1,
          minWidth: 0, // Allow shrinking
          overflow: "hidden", // Prevent overflow
        }}
      >
        {/* Avatar and Chips */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            gap: 2,
            alignItems: "center",
            minWidth: 0, // Allow shrinking
          }}
        >
          <Avatar
            size={40}
            name={peer_id}
            variant="marble"
            colors={["#92A1C6", "#146A7C", "#F0AB3D", "#C271B4", "#C20D90"]}
            style={{ flexShrink: 0 }} // Don't shrink avatar
          />

          {/* Chips Container */}
          <Box
            sx={{
              display: "flex",
              flexDirection: "row",
              gap: 0.5,
              flexWrap: "wrap",
              alignItems: "flex-start",
              minWidth: 0, // Allow shrinking
              flex: 1, // Take remaining space
            }}
          >
            {markup !== null && (
              <Chip
                label={
                  <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                    <Typography variant="body2" component="span">
                      Markup
                    </Typography>
                    <Box
                      sx={{ borderLeft: 1, borderColor: "divider", height: 14 }}
                    />
                    <Typography
                      variant="body2"
                      component="span"
                    >{`${markup.toFixed(1)}%`}</Typography>
                  </Box>
                }
                variant="outlined"
                sx={{
                  color: markup > 20 ? "error.main" : "inherit",
                  borderColor: "divider",
                }}
              />
            )}
            <Chip
              label={
                <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                  <Typography variant="body2" component="span">
                    Min
                  </Typography>
                  <Box
                    sx={{ borderLeft: 1, borderColor: "divider", height: 14 }}
                  />
                  <SatsAmount amount={quote.min_quantity} />
                </Box>
              }
              variant="outlined"
            />
            <Chip
              label={
                <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
                  <Typography variant="body2" component="span">
                    Max
                  </Typography>
                  <Box
                    sx={{ borderLeft: 1, borderColor: "divider", height: 14 }}
                  />
                  <SatsAmount amount={quote.max_quantity} />
                </Box>
              }
              variant="outlined"
            />
            <Chip
              label={<MoneroSatsExchangeRate rate={quote.price} />}
              variant="outlined"
              sx={{ color: "text.secondary", borderColor: "divider" }}
            />
            {isMakerVersionOutdated(version) ? (
              <Tooltip title="Outdated maker version. This may cause issues with the swap.">
                <Chip
                  variant="outlined"
                  label={
                    <Box
                      sx={{ display: "flex", alignItems: "center", gap: 0.5 }}
                    >
                      <WarningIcon sx={{ fontSize: "1rem" }} />
                      <Typography variant="body2">{version}</Typography>
                    </Box>
                  }
                  sx={{
                    color: "warning.main",
                    borderColor: "warning.main",
                  }}
                />
              </Tooltip>
            ) : (
              <Chip
                label={`v${version}`}
                variant="outlined"
                sx={{ color: "text.secondary", borderColor: "divider" }}
              />
            )}
          </Box>
        </Box>

        {/* Address and Peer ID at bottom */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            gap: 0.5,
            minWidth: 0, // Allow shrinking
            width: "100%",
          }}
        >
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              minWidth: 0,
            }}
            title={multiaddr} // Show full address on hover
          >
            {multiaddr}
          </Typography>
        </Box>
      </Box>
      {!noButton && (
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <PromiseInvokeButton
            variant="contained"
            onInvoke={() => resolveApproval(requestId, true)}
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
      )}
    </Paper>
  );
}
