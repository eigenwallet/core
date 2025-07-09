import { Box, Button, Card, Typography } from "@mui/material";
import NumberInput from "./NumberInput";
import SwapVertIcon from '@mui/icons-material/SwapVert';
import { useState } from "react";

interface SendAmountInputProps {
  balance: string;
  amount: string;
  onAmountChange: (amount: string) => void;
}

export default function SendAmountInput({
  balance,
  amount,
  onAmountChange,
}: SendAmountInputProps) {
    const [primaryCurrency, setPrimaryCurrency] = useState<string>("XMR");
  const displayBalance = (parseFloat(balance) / 1000000000000).toFixed(3);

  return (
    <Card
      elevation={2}
      sx={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        width: 500,
        height: 250,
      }}
    >
      <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
        <Box sx={{ display: "flex", alignItems: "baseline", gap: 1 }}>
          <NumberInput
            value={amount}
            onChange={onAmountChange}
            placeholder="0.00"
            fontSize="3em"
            fontWeight={600}
            textAlign="center"
            minWidth={60}
            step={0.001}
            largeStep={0.1}
          />
          <Typography variant="h4" color="text.secondary">
            {primaryCurrency === "XMR" ? "XMR" : "USD"}
          </Typography>
        </Box>
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.5 }}>
            <SwapVertIcon onClick={() => setPrimaryCurrency(primaryCurrency === "XMR" ? "fiat" : "XMR")}/>
            <Typography color="text.secondary">{primaryCurrency === "XMR" ? "0.00 USD" : "0.00 XMR"}</Typography>
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
        <Button variant="secondary" size="tiny">
          Max
        </Button>
      </Box>
    </Card>
  );
}
