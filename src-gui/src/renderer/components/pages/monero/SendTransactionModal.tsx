import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
} from "@mui/material";
import { useState } from "react";
import {
  xmrToPiconeros,
  piconerosToXmr,
} from "../../../../utils/conversionUtils";
import SendAmountInput from "./components/SendAmountInput";

interface SendTransactionModalProps {
  open: boolean;
  onClose: () => void;
  balance: {
    unlocked_balance: string;
  };
  onSend: (transactionData: {
    address: string;
    amount: number;
  }) => Promise<void>;
}

export default function SendTransactionModal({
  balance,
  onSend,
  open,
  onClose,
}: SendTransactionModalProps) {
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
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Send</DialogTitle>
      <DialogContent>
        <SendAmountInput
          balance={balance.unlocked_balance}
          amount={sendAmount}
          onAmountChange={setSendAmount}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <Button onClick={onClose}>Send</Button>
      </DialogActions>
    </Dialog>
  );
}
