import { Box, Button, Card, Grow, Typography } from "@mui/material";
import NumberInput from "renderer/components/inputs/NumberInput";
import SwapVertIcon from "@mui/icons-material/SwapVert";
import { useTheme } from "@mui/material/styles";
import {
  piconerosToXmr,
  satsToBtc,
} from "../../../../../utils/conversionUtils";
import { MoneroAmount, BitcoinAmount } from "renderer/components/other/Units";

interface SendAmountInputProps {
  unlocked_balance: number;
  amount: string;
  onAmountChange: (amount: string) => void;
  onMaxClicked?: () => void;
  onMaxToggled?: () => void;
  currency: string;
  onCurrencyChange: (currency: string) => void;
  wallet: "monero" | "bitcoin";
  walletCurrency: string;
  walletPrecision: number;
  fiatCurrency: string;
  fiatPrice: number | null;
  showFiatRate: boolean;
  disabled?: boolean;
}

export default function SendAmountInput({
  unlocked_balance,
  amount,
  currency,
  wallet,
  walletCurrency,
  walletPrecision,
  onCurrencyChange,
  onAmountChange,
  onMaxClicked,
  onMaxToggled,
  fiatCurrency,
  fiatPrice,
  showFiatRate,
  disabled = false,
}: SendAmountInputProps) {
  const theme = useTheme();
  const baseunitsToFraction = wallet === "monero" ? piconerosToXmr : satsToBtc;
  const estFee =
    wallet === "monero" ? 10000000000 /* 0.01 XMR */ : 10000; /* 0.0001 BTC */
  const walletStep = wallet === "monero" ? 0.001 : 0.00001;
  const walletLargeStep = wallet === "monero" ? 0.1 : 0.001;
  const WalletAmount = wallet === "monero" ? MoneroAmount : BitcoinAmount;

  const isMaxSelected = amount === "<MAX>";

  // Calculate secondary amount for display
  const secondaryAmount = (() => {
    if (isMaxSelected) {
      return "All available funds";
    }

    if (!amount || amount === "" || isNaN(parseFloat(amount))) {
      return "0.00";
    }

    if (fiatPrice === null) {
      return "?";
    }

    const primaryValue = parseFloat(amount);
    if (currency === walletCurrency) {
      // Primary is XMR, secondary is USD
      return (primaryValue * fiatPrice).toFixed(2);
    } else {
      // Primary is USD, secondary is XMR
      return (primaryValue / fiatPrice).toFixed(walletPrecision);
    }
  })();

  const handleMaxAmount = () => {
    if (disabled) return;
    if (onMaxToggled) {
      onMaxToggled();
    } else if (onMaxClicked) {
      onMaxClicked();
    } else {
      // Fallback to old behavior if no callback provided
      // TODO: We need to use a real fee here and call sweep(...) instead of just subtracting a fixed amount
      const maxWalletAmount = baseunitsToFraction(unlocked_balance - estFee); // Subtract ~0.01 XMR/~0.0001 BTC for fees

      if (currency === walletCurrency) {
        onAmountChange(Math.max(0, maxWalletAmount).toString());
      } else if (fiatPrice !== null) {
        // Convert to USD for display
        const maxAmountUsd = maxWalletAmount * fiatPrice;
        onAmountChange(Math.max(0, maxAmountUsd).toString());
      }
    }
  };

  const handleMaxTextClick = () => {
    if (disabled) return;
    if (isMaxSelected && onMaxToggled) {
      onMaxToggled();
    }
  };

  const handleCurrencySwap = () => {
    if (!isMaxSelected && !disabled) {
      onCurrencyChange(
        currency === walletCurrency ? fiatCurrency : walletCurrency,
      );
    }
  };

  const isAmountTooHigh =
    !isMaxSelected &&
    (currency === walletCurrency
      ? parseFloat(amount) > baseunitsToFraction(unlocked_balance)
      : fiatPrice !== null &&
        parseFloat(amount) / fiatPrice > baseunitsToFraction(unlocked_balance));

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
        opacity: disabled ? 0.6 : 1,
        pointerEvents: disabled ? "none" : "auto",
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
          {isMaxSelected ? (
            <Typography
              variant="h3"
              onClick={handleMaxTextClick}
              sx={{
                fontWeight: 600,
                color: "primary.main",
                cursor: disabled ? "default" : "pointer",
                userSelect: "none",
                "&:hover": {
                  opacity: disabled ? 1 : 0.8,
                },
              }}
              title={disabled ? "" : "Click to edit amount"}
            >
              &lt;MAX&gt;
            </Typography>
          ) : (
            <>
              <NumberInput
                value={amount}
                onChange={disabled ? () => {} : onAmountChange}
                placeholder={(0).toFixed(
                  currency === walletCurrency ? walletPrecision : 2,
                )}
                fontSize="3em"
                fontWeight={600}
                minWidth={60}
                step={currency === walletCurrency ? walletStep : 0.01}
                largeStep={currency === walletCurrency ? walletLargeStep : 10}
              />
              <Typography variant="h4" color="text.secondary">
                {currency}
              </Typography>
            </>
          )}
        </Box>
        {showFiatRate && (
          <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <SwapVertIcon
              onClick={handleCurrencySwap}
              sx={{
                cursor: isMaxSelected || disabled ? "default" : "pointer",
                opacity: isMaxSelected || disabled ? 0.5 : 1,
              }}
            />
            <Typography color="text.secondary">
              {secondaryAmount}{" "}
              {isMaxSelected
                ? ""
                : currency === walletCurrency
                  ? fiatCurrency
                  : walletCurrency}
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
        <Typography color="text.primary">
          <WalletAmount amount={baseunitsToFraction(unlocked_balance)} />
        </Typography>
        <Button
          variant={isMaxSelected ? "contained" : "secondary"}
          size="tiny"
          onClick={handleMaxAmount}
          disabled={disabled}
        >
          Max
        </Button>
      </Box>
    </Card>
  );
}
