import { useState } from "react";
import {
  Typography,
  TextField,
  Button,
  Card,
  CardContent,
  InputAdornment,
  Stack,
  Divider,
} from "@mui/material";
import { Send as SendIcon } from "@mui/icons-material";
import PromiseInvokeButton from "../../../PromiseInvokeButton";
import {
  xmrToPiconeros,
  piconerosToXmr,
} from "../../../../../utils/conversionUtils";

interface SendTransactionProps {
  balance?: {
    unlocked_balance: string;
  };
  onSend: (transactionData: {
    address: string;
    amount: number;
  }) => Promise<void>;
}

// Component for sending transactions
export default function SendTransaction({
  balance,
  onSend,
}: SendTransactionProps) {
  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");

  const handleSend = async () => {
    if (!sendAddress || !sendAmount) {
      throw new Error("Address and amount are required");
    }

    return onSend({
      address: sendAddress,
      amount: xmrToPiconeros(parseFloat(sendAmount)),
    });
  };

  const handleSendSuccess = () => {
    // Clear form after successful send
    setSendAddress("");
    setSendAmount("");
  };

  const handleMaxAmount = () => {
    if (balance?.unlocked_balance) {
      // TODO: We need to use a real fee here and call sweep(...) instead of just subtracting a fixed amount
      const unlocked = parseFloat(balance.unlocked_balance);
      const maxAmount = piconerosToXmr(unlocked - 10000000000); // Subtract ~0.01 XMR for fees
      setSendAmount(Math.max(0, maxAmount).toString());
    }
  };

  const handleClear = () => {
    setSendAddress("");
    setSendAmount("");
  };

  return (
    <Card>
      <CardContent sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Typography variant="h6">Transfer</Typography>
        <Divider />
        <Stack spacing={2}>
          <TextField
            fullWidth
            label="Pay to"
            placeholder="Monero address"
            value={sendAddress}
            onChange={(e) => setSendAddress(e.target.value)}
          />

          <Stack direction="row" spacing={1}>
            <TextField
              fullWidth
              label="Amount"
              placeholder="0.0"
              value={sendAmount}
              onChange={(e) => setSendAmount(e.target.value)}
              type="number"
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">XMR</InputAdornment>
                ),
              }}
            />
            <Button
              variant="outlined"
              onClick={handleMaxAmount}
              disabled={!balance?.unlocked_balance}
            >
              Max
            </Button>
          </Stack>

          <Stack direction="row" spacing={1} justifyContent="flex-end">
            <Button variant="outlined" onClick={handleClear}>
              Clear
            </Button>
            <PromiseInvokeButton
              variant="contained"
              color="primary"
              endIcon={<SendIcon />}
              onInvoke={handleSend}
              onSuccess={handleSendSuccess}
              disabled={!sendAddress || !sendAmount}
              displayErrorSnackbar={true}
              sx={{ minWidth: 100 }}
            >
              Send
            </PromiseInvokeButton>
          </Stack>
        </Stack>
      </CardContent>
    </Card>
  );
}
