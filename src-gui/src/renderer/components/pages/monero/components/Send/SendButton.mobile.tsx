import { Box, Button, SwipeableDrawer, Typography, useTheme } from "@mui/material";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import { useState } from "react";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import { SendMoneroResponse } from "models/tauriModel";
import { useAppSelector, usePendingSendMoneroApproval } from "store/hooks";
import SendTransactionContent from "./SendTransactionContent";
import SendApprovalContent from "./SendApprovalContent";
import SendSuccessContent from "./SendSuccessContent";
import MobileDialog from "renderer/components/modal/MobileDialog";
import MobileDialogHeader from "renderer/components/modal/MobileDialogHeader";
import { useCreateSendTransaction } from "utils/useCreateSendTransaction";
import SendAmountInput from "./SendAmountInput";
import PromiseInvokeButton from "renderer/components/buttons/PromiseInvokeButton";
import SendEnterAddressContent from "./SendEnterAdressContent";
import SendEnterAmountContent from "./SendEnterAmountContent";

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
    handleClear,
  } = useCreateSendTransaction(setSuccessResponse);

  const pendingApprovals = usePendingSendMoneroApproval();
  const hasPendingApproval = pendingApprovals.length > 0;


  const showSuccess = successResponse !== null;

  let step: SendTransactionState;
  if (hasPendingApproval) {
    step = SendTransactionState.ApprovalPending
  } else if (showSuccess) {
    step = SendTransactionState.Success
  } else if (!addressConfirmed) {
    step = SendTransactionState.EnterAddress
  } else if (addressConfirmed) {
    step = SendTransactionState.EnterAmount
  }

  const handleClose = () => {
    setAddressConfirmed(false);
    handleClear();
    setOpen(false);
    setSuccessResponse(null);
  };

  return (
    <>
      <TextIconButton label="Send" onClick={() => setOpen(true)} disabled={disabled} isMainActionButton>
        <ArrowUpwardIcon />
      </TextIconButton>
      <SwipeableDrawer open={open} onOpen={() => setOpen(true)} onClose={handleClose} anchor="bottom" disableSwipeToOpen={true} slotProps={{ paper: { sx: {borderTopLeftRadius: 16, borderTopRightRadius: 16 } } }}>
        <Box
          sx={{
            minHeight: "70vh",
            display: "flex",
            alignItems: "stretch",
            flexDirection: "column",
            gap: 2,
            p: 2,
            pb: 8,
          }}
        >
          {step === SendTransactionState.EnterAddress && (
            <SendEnterAddressContent
              open={open}
              onContinue={() => setAddressConfirmed(true)}
              address={sendAddress}
              onAddressChange={handleAddressChange}
              onAddressValidityChange={setValidAddress}
            />
          )}
          {step === SendTransactionState.EnterAmount && (
            <SendEnterAmountContent
              balance={balance}
              sendAmount={sendAmount}
              onAmountChange={handleAmountChange}
              onMaxToggled={handleMaxToggled}
              currency={currency}
              onCurrencyChange={handleCurrencyChange}
              isSending={isSending}
              isSendDisabled={isSendDisabled}
              onSend={handleSend}
              onSendSuccess={handleSendSuccess}
              onPendingChange={setIsSending}
            />
          )}
          {hasPendingApproval && (
            <SendApprovalContent onClose={handleClose} />
          )}
          {step === SendTransactionState.Success && (
            <SendSuccessContent onClose={handleClose} successDetails={successResponse} />
          )}
        </Box>
      </SwipeableDrawer>
    </>
  );
}
