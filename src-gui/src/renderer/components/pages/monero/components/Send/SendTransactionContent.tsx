import {
  Button,
  Box,
  DialogActions,
  DialogContent,
  DialogTitle,
  useTheme,
} from "@mui/material";
import SendAmountInput from "./SendAmountInput";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import PromiseInvokeButton from "renderer/components/buttons/PromiseInvokeButton";
import { SendMoneroResponse } from "models/tauriModel";
import { useCreateSendTransaction } from "utils/useCreateSendTransaction";

interface SendTransactionContentProps {
  balance: {
    unlocked_balance: string;
  };
  onClose: () => void;
  onSuccess: (response: SendMoneroResponse) => void;
}

export default function SendTransactionContent({
  balance,
  onSuccess,
  onClose,
}: SendTransactionContentProps) {
  const {
    sendAddress,
    handleAddressChange,
    sendAmount,
    handleAmountChange,
    handleMaxToggled,
    currency,
    handleCurrencyChange,
    isSending,
    isSendDisabled,
    setValidAddress,
    handleSend,
    setIsSending,
    handleSendSuccess,
  } = useCreateSendTransaction(onSuccess);

  const theme = useTheme();

  if (!balance || !balance.unlocked_balance) {
    return <></>
  }

  return (
    <>
      <DialogTitle>Send</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <SendAmountInput
            balance={balance}
            amount={sendAmount}
            onAmountChange={handleAmountChange}
            onMaxToggled={handleMaxToggled}
            currency={currency}
            onCurrencyChange={handleCurrencyChange}
            disabled={isSending}
            sx={{
              border: `1px solid ${theme.palette.grey[800]}`,
            }}
          />
          <MoneroAddressTextField
            address={sendAddress}
            onAddressChange={handleAddressChange}
            onAddressValidityChange={setValidAddress}
            label="Send to"
            fullWidth
            disabled={isSending}
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleSend}
          disabled={isSendDisabled}
          onSuccess={handleSendSuccess}
          onPendingChange={setIsSending}
        >
          Send
        </PromiseInvokeButton>
      </DialogActions>
    </>
  );
}
