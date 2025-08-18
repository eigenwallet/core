import { Box, Card, Typography, useTheme } from "@mui/material";

import { CardContent } from "@mui/material";
import { SatsAmount } from "renderer/components/other/Units";
import BitcoinIcon from "renderer/components/icons/BitcoinIcon";

export default function BitcoinWalletOverview({
  bitcoinBalance,
}: {
  bitcoinBalance: number;
}) {
  const theme = useTheme();
  return (
    <Card
      sx={{
        background:
          theme.palette.mode === "dark" ? "rgba(255,255,255,0.06)" : "#fafafa",
        borderRadius: 3,
      }}
    >
      <CardContent
        sx={{
          display: "flex",
          alignItems: "center",
          p: 2,
          "&:last-child": { pb: 2 },
          justifyContent: "space-between",
        }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <BitcoinIcon
            sx={{
              fontSize: 16,
              color: theme.palette.mode === "dark" ? "#FF6600" : "#FF6600",
            }}
          />
          <Typography variant="subtitle2" color="text.secondary">
            Bitcoin
          </Typography>
        </Box>
        <Typography variant="subtitle1" fontWeight={600} sx={{ mr: 1 }}>
          {bitcoinBalance !== null ? (
            <SatsAmount amount={bitcoinBalance} />
          ) : (
            "--"
          )}
        </Typography>
      </CardContent>
    </Card>
  );
}
