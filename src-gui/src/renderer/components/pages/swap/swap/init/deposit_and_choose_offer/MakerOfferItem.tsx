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
  const isOutOfLiquidity = quote.max_quantity == 0;

  return (
    <Paper
      variant="outlined"
      sx={{
        position: "relative",
        display: "flex",
        flexDirection: { xs: "column", sm: "row" },
        gap: 2,
        borderRadius: 2,
        padding: 2,
        width: "100%",
        justifyContent: "space-between",
        alignItems: { xs: "stretch", sm: "center" },
      }}
    >
      <Box
        sx={{
          display: "flex",
          flexDirection: "row",
          gap: 2,
        }}
      >
        <Avatar
          size={40}
          name={peer_id}
          variant="marble"
          colors={["#92A1C6", "#146A7C", "#F0AB3D", "#C271B4", "#C20D90"]}
        />
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            gap: 1,
          }}
        >
          <Typography variant="body1" sx={{ maxWidth: "200px" }} noWrap>
            {multiaddr}
          </Typography>
          <Typography variant="body1" sx={{ maxWidth: "200px" }} noWrap>
            {peer_id}
          </Typography>
          <Box
            sx={{
              display: "flex",
              flexDirection: { xs: "column", sm: "row" },
              gap: 1,
              flexWrap: "wrap",
            }}
          >
            <Chip
              label={
                <MoneroSatsExchangeRate
                  rate={quote.price}
                  displayMarkup={true}
                />
              }
              size="small"
            />
            <Chip
              label={
                <>
                  <SatsAmount amount={quote.min_quantity} /> -{" "}
                  <SatsAmount amount={quote.max_quantity} />
                </>
              }
              size="small"
            />
            {isMakerVersionOutdated(version) ? (
              <Tooltip title="Outdated maker version. This may cause issues with the swap.">
                <Chip
                  color="warning"
                  label={
                    <Box
                      sx={{ display: "flex", alignItems: "center", gap: 0.5 }}
                    >
                      <WarningIcon sx={{ fontSize: "1rem" }} />
                      <Typography variant="body2">{version}</Typography>
                    </Box>
                  }
                  size="small"
                />
              </Tooltip>
            ) : (
              <Chip label={version} size="small" />
            )}
          </Box>
        </Box>
      </Box>
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
