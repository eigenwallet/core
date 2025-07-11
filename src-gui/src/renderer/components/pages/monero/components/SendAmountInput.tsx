import { Box, Button, Card, Typography } from "@mui/material";
import NumberInput from "../../../inputs/NumberInput";
import SwapVertIcon from "@mui/icons-material/SwapVert";
import { useState } from "react";
import { useTheme } from "@mui/material/styles";
import { piconerosToXmr } from "../../../../../utils/conversionUtils";

interface SendAmountInputProps {
  balance: {
    unlocked_balance: string;
  };
  amount: string;
  onAmountChange: (amount: string) => void;
}

export default function SendAmountInput({
  balance,
  amount,
  onAmountChange,
}: SendAmountInputProps) {
  const theme = useTheme();
  const [primaryCurrency, setPrimaryCurrency] = useState<string>("XMR");
  const displayBalance = (parseFloat(balance.unlocked_balance) / 1000000000000).toFixed(3);

  const handleMaxAmount = () => {
    if (balance?.unlocked_balance) {
      // TODO: We need to use a real fee here and call sweep(...) instead of just subtracting a fixed amount
      const unlocked = parseFloat(balance.unlocked_balance);
      const maxAmount = piconerosToXmr(unlocked - 10000000000); // Subtract ~0.01 XMR for fees
      onAmountChange(Math.max(0, maxAmount).toString());
    }
  };

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
        width: 500,
        height: 250,
      }}
    >
      <Box
        sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}
      >
        <Box sx={{ display: "flex", alignItems: "baseline", gap: 1 }}>
          <NumberInput
            value={amount}
            onChange={onAmountChange}
            placeholder="0.00"
            fontSize="3em"
            fontWeight={600}
            minWidth={60}
            step={0.001}
            largeStep={0.1}
          />
          <Typography variant="h4" color="text.secondary">
            {primaryCurrency === "XMR" ? "XMR" : "USD"}
          </Typography>
        </Box>
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
          <SwapVertIcon
            onClick={() =>
              setPrimaryCurrency(primaryCurrency === "XMR" ? "fiat" : "XMR")
            }
          />
          <Typography color="text.secondary">
            {primaryCurrency === "XMR" ? "0.00 USD" : "0.00 XMR"}
          </Typography>
        </Box>
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
