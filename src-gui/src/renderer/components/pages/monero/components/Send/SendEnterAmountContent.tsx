import { Box, Typography } from "@mui/material";
import SendAmountInput from "./SendAmountInput";
import PromiseInvokeButton from "renderer/components/buttons/PromiseInvokeButton";
import { SendMoneroResponse } from "models/tauriModel";

interface SendEnterAmountContentProps {
  balance: {
    unlocked_balance: string;
  };
  sendAmount: string;
  onAmountChange: (amount: string) => void;
  onMaxToggled: () => void;
  currency: string;
  onCurrencyChange: (currency: string) => void;
  isSending: boolean;
  isSendDisabled: boolean;
  onSend: () => Promise<SendMoneroResponse>;
  onSendSuccess: (response: SendMoneroResponse) => void;
  onPendingChange: (isPending: boolean) => void;
}

export default function SendEnterAmountContent({
  balance,
  sendAmount,
  onAmountChange,
  onMaxToggled,
  currency,
  onCurrencyChange,
  isSending,
  isSendDisabled,
  onSend,
  onSendSuccess,
  onPendingChange,
}: SendEnterAmountContentProps) {
  return (
    <Box sx={{ display: "flex", flex: 1, flexDirection: "column", justifyContent: "space-between", alignItems: "center" }}>
      <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3, width: '100%' }}>
      <Typography variant="h6" sx={{ textAlign: "center" }}>Choose Amount</Typography>
      <SendAmountInput
        balance={balance}
        amount={sendAmount}
        onAmountChange={onAmountChange}
        onMaxToggled={onMaxToggled}
        currency={currency}
        onCurrencyChange={onCurrencyChange}
        disabled={isSending}
        sx={{
            bgcolor: "transparent",
            minHeight: 180,
            justifyContent: "space-between",
            my: 5,
        }}
        />
    </Box>
      <PromiseInvokeButton 
        variant="contained" 
        onInvoke={onSend} 
        onSuccess={onSendSuccess} 
        onPendingChange={onPendingChange}
        disabled={isSendDisabled}
      >
        Continue
      </PromiseInvokeButton>
    </Box>
  );
}
