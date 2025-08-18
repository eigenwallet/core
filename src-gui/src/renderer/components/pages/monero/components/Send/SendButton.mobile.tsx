import { Box, Button, SwipeableDrawer, Typography, useTheme } from "@mui/material";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import { useState } from "react";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import { SendMoneroResponse } from "models/tauriModel";
import { useAppSelector, usePendingSendMoneroApproval } from "store/hooks";
import SendTransactionContent from "renderer/components/features/wallet/Send/SendTransactionContent";
import SendApprovalContent from "renderer/components/pages/monero/components/SendApprovalContent";
import SendSuccessContent from "renderer/components/pages/monero/components/SendSuccessContent";
import MobileDialog from "renderer/components/modal/MobileDialog";
import MobileDialogHeader from "renderer/components/modal/MobileDialogHeader";
import { useCreateSendTransaction } from "utils/useCreateSendTransaction";
import SendAmountInput from "renderer/components/pages/monero/components/SendAmountInput";
import PromiseInvokeButton from "renderer/components/buttons/PromiseInvokeButton";

enum SendTransactionState {
  EnterAddress,
  EnterAmount,
  ApprovalPending,
  Success,
}

export default function SendButton({
  balance,
  disabled,
}: {
  balance: {
    unlocked_balance: string;
  };
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [successResponse, setSuccessResponse] =
    useState<SendMoneroResponse | null>(null);
  const [addressConfirmed, setAddressConfirmed] = useState(false);
  
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
  } = useCreateSendTransaction(setSuccessResponse);

  const pendingApprovals = usePendingSendMoneroApproval();
  const hasPendingApproval = pendingApprovals.length > 0;

  const fiatCurrency = useAppSelector((state) => state.settings.fiatCurrency);
  const showFiatRate = useAppSelector(
    (state) => state.settings.fetchFiatPrices,
  );
  const xmrPrice = useAppSelector((state) => state.rates.xmrPrice);

  const theme = useTheme();

  const showSuccess = successResponse !== null;

  let sendTransactionState: SendTransactionState, label: string;
  if (hasPendingApproval) {
    sendTransactionState = SendTransactionState.ApprovalPending
    label = "Confirm"
  } else if (showSuccess) {
    sendTransactionState = SendTransactionState.Success
    label = "Succesfully Sent"
  } else if (!sendAddress) {
    sendTransactionState = SendTransactionState.EnterAddress
    label = "Select Recepient"
  } else if (addressConfirmed) {
    sendTransactionState = SendTransactionState.EnterAmount
    label = "Choose Amount"
  }

  const handleClose = () => {
    setOpen(false);
    setSuccessResponse(null);
  };

  return (
    <>
      <TextIconButton label="Send" onClick={() => setOpen(true)} disabled={disabled} isMainActionButton>
        <ArrowUpwardIcon />
      </TextIconButton>
      <SwipeableDrawer open={open} onOpen={() => setOpen(true)} onClose={handleClose} anchor="bottom" disableSwipeToOpen={true} slotProps={{ paper: { sx: { minHeight: "90vh", borderTopLeftRadius: 16, borderTopRightRadius: 16 } } }}>
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 2,
            p: 2,
            pb: 8,
          }}
        >
          {sendTransactionState === SendTransactionState.EnterAddress && (
            <SendTransactionContent
              balance={balance}
              onClose={handleClose}
              onSuccess={setSuccessResponse}
            />
          )}
          {sendTransactionState === SendTransactionState.EnterAmount && (
            <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 2, width: "100%" }}>
              <Typography variant="h6" sx={{ textAlign: "center" }}>Choose Amount</Typography>
              <SendAmountInput
              balance={balance}
              amount={sendAmount}
              onAmountChange={handleAmountChange}
              onMaxToggled={handleMaxToggled}
              currency={currency}
              fiatCurrency={fiatCurrency}
              xmrPrice={xmrPrice}
              showFiatRate={showFiatRate}
              onCurrencyChange={handleCurrencyChange}
              disabled={isSending}
              sx={{
                bgcolor: "transparent",
                minHeight: 180,
                justifyContent: "space-between",
                my: 5,
              }}
            />
            <PromiseInvokeButton variant="contained" onInvoke={handleSend} onSuccess={handleSendSuccess} onPendingChange={setIsSending}
              disabled={isSendDisabled}
            >
              Continue
            </PromiseInvokeButton>
            </Box>
          )}
          {sendTransactionState === SendTransactionState.ApprovalPending && (
            <SendApprovalContent onClose={handleClose} />
          )}
          {sendTransactionState === SendTransactionState.Success && (
            <SendSuccessContent onClose={handleClose} successDetails={successResponse} />
          )}
        </Box>
      </SwipeableDrawer>
    </>
  );
}
