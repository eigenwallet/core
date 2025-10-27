import { Box, Typography, Card } from "@mui/material";
import { BitcoinAmount } from "renderer/components/other/Units";
import { useAppSelector, useSettings } from "store/hooks";
import { satsToBtc } from "utils/conversionUtils";
import WalletRefreshButton from "../WalletRefreshButton";

interface WalletOverviewProps {
  balance: number | null;
}

function FiatBitcoinAmount({ amount }: { amount: number | null }) {
  const btcPrice = useAppSelector((state) => state.rates.btcPrice);
  const [fetchFiatPrices, fiatCurrency] = useSettings((settings) => [
    settings.fetchFiatPrices,
    settings.fiatCurrency,
  ]);

  if (
    !fetchFiatPrices ||
    fiatCurrency == null ||
    amount == null ||
    btcPrice == null
  ) {
    return <span />;
  }

  return (
    <span>
      {(amount * btcPrice).toFixed(2)} {fiatCurrency}
    </span>
  );
}

export default function WalletOverview({ balance }: WalletOverviewProps) {
  const btcBalance = balance == null ? null : satsToBtc(balance);

  return (
    <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
      <Box
        sx={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "stretch",
          mb: 1,
        }}
      >
        {/* Left side content */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            gap: 0.5,
          }}
        >
          <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
            Available Funds
          </Typography>
          <Typography variant="h4">
            <BitcoinAmount amount={btcBalance} />
          </Typography>
          <Typography variant="body2" color="text.secondary">
            <FiatBitcoinAmount amount={btcBalance} />
          </Typography>
        </Box>

        {/* Right side - Refresh button */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
            justifyContent: "flex-start",
          }}
        >
          <WalletRefreshButton />
        </Box>
      </Box>
    </Card>
  );
}
