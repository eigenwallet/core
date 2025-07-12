import { Box, Button, Card, Grow, Typography } from "@mui/material";
import NumberInput from "../../../inputs/NumberInput";
import SwapVertIcon from "@mui/icons-material/SwapVert";
import { useTheme } from "@mui/material/styles";
import { piconerosToXmr } from "../../../../../utils/conversionUtils";

interface SendAmountInputProps {
  balance: {
    unlocked_balance: string;
  };
  amount: string;
  onAmountChange: (amount: string) => void;
  currency: string;
  onCurrencyChange: (currency: string) => void;
  fiatCurrency: string;
  xmrPrice: number;
  showFiatRate: boolean;
}

export default function SendAmountInput({
  balance,
  amount,
  currency,
  onCurrencyChange,
  onAmountChange,
  fiatCurrency,
  xmrPrice,
  showFiatRate,
}: SendAmountInputProps) {
  const theme = useTheme();
  const displayBalance = piconerosToXmr(parseFloat(balance.unlocked_balance)).toFixed(3);

  // Calculate secondary amount for display
  const secondaryAmount = (() => {
    if (!amount || amount === "" || isNaN(parseFloat(amount))) {
      return "0.00";
    }

    const primaryValue = parseFloat(amount);
    if (currency === "XMR") {
      // Primary is XMR, secondary is USD
      return (primaryValue * xmrPrice).toFixed(2);
    } else {
      // Primary is USD, secondary is XMR
      return (primaryValue / xmrPrice).toFixed(3);
    }
  })();

  const handleMaxAmount = () => {
    if (
      balance?.unlocked_balance !== undefined &&
      balance?.unlocked_balance !== null
    ) {
      // TODO: We need to use a real fee here and call sweep(...) instead of just subtracting a fixed amount
      const unlocked = parseFloat(balance.unlocked_balance);
      const maxAmountXmr = piconerosToXmr(unlocked - 10000000000); // Subtract ~0.01 XMR for fees

      if (currency === "XMR") {
        onAmountChange(Math.max(0, maxAmountXmr).toString());
      } else {
        // Convert to USD for display
        const maxAmountUsd = maxAmountXmr * xmrPrice;
        onAmountChange(Math.max(0, maxAmountUsd).toString());
      }
    }
  };

  const handleCurrencySwap = () => {
    onCurrencyChange(currency === "XMR" ? fiatCurrency : "XMR");
  };

  const isAmountTooHigh =
    currency === "XMR"
      ? parseFloat(amount) > piconerosToXmr(parseFloat(balance.unlocked_balance))
      : parseFloat(amount) / xmrPrice > piconerosToXmr(parseFloat(balance.unlocked_balance));

  return (
    <Card
      elevation={0}
      tabIndex={0}
      sx={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        border: `1px solid ${theme.palette.grey[800]}`,
        width: "100%",
        height: 250,
      }}
    >
      <Box
        sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}
      >
        {isAmountTooHigh && (
          <Grow
            in
            style={{ transitionDelay: isAmountTooHigh ? "100ms" : "0ms" }}
          >
            <Typography variant="caption" align="center" color="error">
              You don't have enough
              <br /> unlocked balance to send this amount.
            </Typography>
          </Grow>
        )}
        <Box sx={{ display: "flex", alignItems: "baseline", gap: 1 }}>
          <NumberInput
            value={amount}
            onChange={onAmountChange}
            placeholder={currency === "XMR" ? "0.000" : "0.00"}
            fontSize="3em"
            fontWeight={600}
            minWidth={60}
            step={currency === "XMR" ? 0.001 : 0.01}
            largeStep={currency === "XMR" ? 0.1 : 10}
          />
          <Typography variant="h4" color="text.secondary">
            {currency}
          </Typography>
        </Box>
        {showFiatRate && (
          <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <SwapVertIcon
              onClick={handleCurrencySwap}
              sx={{ cursor: "pointer" }}
            />
            <Typography color="text.secondary">
              {secondaryAmount} {currency === "XMR" ? fiatCurrency : "XMR"}
            </Typography>
          </Box>
        )}
      </Box>

      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          width: "100%",
          justifyContent: "center",
          gap: 1.5,
          position: "absolute",
          bottom: 12,
          left: 0,
        }}
      >
        <Typography color="text.secondary">Available</Typography>
        <Box sx={{ display: "flex", alignItems: "baseline", gap: 0.5 }}>
          <Typography color="text.primary">{displayBalance}</Typography>
          <Typography color="text.secondary">XMR</Typography>
        </Box>
        <Button variant="secondary" size="tiny" onClick={handleMaxAmount}>
          Max
        </Button>
      </Box>
    </Card>
  );
}
