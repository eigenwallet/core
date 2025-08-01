import React from "react";
import { Box, Typography, Card, CardContent, useTheme } from "@mui/material";
import {
  PiconeroAmount,
  FiatPiconeroAmount,
} from "renderer/components/other/Units";
import MoneroIcon from "renderer/components/icons/MoneroIcon";
import { GetMoneroBalanceResponse } from "models/tauriModel";

interface MoneroWalletOverviewProps {
  balance: GetMoneroBalanceResponse | null;
}

/**
 * Mobile-optimized Monero wallet overview component
 * Displays balance information in a compact card format
 */
export default function MoneroWalletOverview({
  balance,
}: MoneroWalletOverviewProps) {
  const theme = useTheme();

  return (
    <Card
      sx={{
        background:
          theme.palette.mode === "dark" ? "rgba(255,255,255,0.08)" : "#f5f5f5",
        borderRadius: 3,
      }}
    >
      <CardContent
        sx={{
          display: "flex",
          flexDirection: "row",
          alignItems: "flex-end",
          p: 2,
          "&:last-child": { pb: 2 },
          justifyContent: "space-between",
        }}
      >
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1.5 }}>
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <MoneroIcon
              sx={{
                fontSize: 16,
                color: theme.palette.mode === "dark" ? "#FF6600" : "#FF6600",
              }}
            />
            <Typography variant="subtitle2" color="text.secondary">
              Monero
            </Typography>
          </Box>
          <Typography variant="caption" color="text.secondary">
            {balance && (
              <FiatPiconeroAmount
                amount={parseFloat(balance.unlocked_balance)}
              />
            )}
          </Typography>
        </Box>
        <Box>
          <Typography variant="h4" fontWeight={700} sx={{ mr: 1 }}>
            {balance ? (
              <PiconeroAmount
                amount={parseFloat(balance.unlocked_balance)}
                fixedPrecision={4}
                disableTooltip
                labelStyles={{ fontSize: 24 }}
              />
            ) : (
              "--"
            )}
          </Typography>
        </Box>
      </CardContent>
    </Card>
  );
}
