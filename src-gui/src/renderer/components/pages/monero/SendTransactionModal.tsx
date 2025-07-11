import {
  Button,
  Box,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
} from "@mui/material";
import { useState } from "react";
import { xmrToPiconeros } from "../../../../utils/conversionUtils";
import SendAmountInput from "./components/SendAmountInput";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { sendMoneroTransaction } from "renderer/rpc";

interface SendTransactionModalProps {
  open: boolean;
  onClose: () => void;
  balance: {
    unlocked_balance: string;
  };
}

export default function SendTransactionModal({
  balance,
  open,
  onClose,
}: SendTransactionModalProps) {
  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");
  const [enableSend, setEnableSend] = useState(false);

  const handleSend = async () => {
    if (!sendAddress || !sendAmount) {
      throw new Error("Address and amount are required");
    }

    return sendMoneroTransaction({
      address: sendAddress,
      amount: xmrToPiconeros(parseFloat(sendAmount)),
    });
  };


  const handleSendSuccess = () => {
    // Clear form after successful send
    handleClear();
    onClose();
  };

  const handleClear = () => {
    setSendAddress("");
    setSendAmount("");
  };

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Send</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <SendAmountInput
          balance={balance}
          amount={sendAmount}
          onAmountChange={setSendAmount}
        />
        <MoneroAddressTextField
          address={sendAddress}
          onAddressChange={setSendAddress}
          onAddressValidityChange={setEnableSend}
          label="Send to"
          fullWidth
        />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleSend}
          disabled={!enableSend}
          onSuccess={handleSendSuccess}
        >
          Send
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
