import { Box, Button, Typography, Alert } from "@mui/material";
import { useState } from "react";
import IntercambioAmountInput from "./IntercambioAmountInput";
import { intercambioTrade } from "renderer/rpc";
import { useSnackbar } from "notistack";
import CircularProgressWithSubtitle from "../swap/swap/components/CircularProgressWithSubtitle";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import LoadingButton from "renderer/components/other/LoadingButton";
import { useAppSelector } from "store/hooks";
import { btcToSats, satsToBtc } from "utils/conversionUtils";

export default function IntercambioPage() {
  const [btcAmount, setBtcAmount] = useState("0.001");
  const [isLoading, setIsLoading] = useState(false);
  const [depositAddress, setDepositAddress] = useState<string | null>(null);
  const [returnXmr, setReturnXmr] = useState<number | null>(null);
  const { enqueueSnackbar } = useSnackbar();

  const xmrBtcRate = useAppSelector((state) => state.rates.xmrBtcRate);
  const estimatedXmr = xmrBtcRate ? parseFloat(btcAmount) / xmrBtcRate : undefined;

  const handleStartSwap = async () => {
    try {
      setIsLoading(true);
      const { deposit_address, xmr_amount } = await intercambioTrade(btcAmount);
      setDepositAddress(deposit_address);
      setReturnXmr(xmr_amount);
    } catch (error) {
      enqueueSnackbar(String(error), { variant: "error" });
    } finally {
      setIsLoading(false);
    }
  };

  const handleReset = () => {
    setDepositAddress(null);
    setBtcAmount("0.001");
  };

  if (isLoading) {
    return (
      <Box
        sx={{
          display: "flex",
          justifyContent: "center",
          alignItems: "center",
          minHeight: "50vh",
        }}
      >
        <CircularProgressWithSubtitle description="Creating trade..." />
      </Box>
    );
  }

  if (depositAddress) {
    return (
      <Box
        sx={{
          maxWidth: 600,
          mx: "auto",
          display: "flex",
          flexDirection: "column",
          gap: 3,
        }}
      >
        <Alert severity="success">
          Trade created successfully! Send Bitcoin to the address below.
        </Alert>

        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <Typography variant="h6">Deposit Address</Typography>
          <Typography variant="body2" color="text.secondary">
            You can deposit the money to the following address:
          </Typography>
          <ActionableMonospaceTextBox
            content={depositAddress}
            displayCopyIcon={true}
            enableQrCode={true}
          />
        </Box>

        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <Typography variant="body2" color="text.secondary">
            Amount: <strong>{btcAmount} BTC</strong>
          </Typography>
          <Typography variant="body2" color="text.secondary">
            You will receive: <strong>{returnXmr} XMR</strong>
          </Typography>
        </Box>

        <Button variant="outlined" size="large" fullWidth onClick={handleReset}>
          Create New Trade
        </Button>
      </Box>
    );
  }

  return (
    <Box
      sx={{
        maxWidth: 600,
        mx: "auto",
        display: "flex",
        flexDirection: "column",
        gap: 2,
      }}
    >
      <IntercambioAmountInput
        btcAmount={btcAmount}
        onBtcAmountChange={setBtcAmount}
        estimatedXmr={estimatedXmr}
      />

      <LoadingButton
        variant="contained"
        size="large"
        fullWidth
        onClick={handleStartSwap}
        loading={isLoading}
      >
        Start Swap
      </LoadingButton>
    </Box>
  );
}
