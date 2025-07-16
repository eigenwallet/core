import { Dialog } from "@mui/material";
import SendTransactionContent from "./components/SendTransactionContent";
import SendApprovalContent from "./components/SendApprovalContent";
import { useState } from "react";
import SendSuccessContent from "./components/SendSuccessContent";
import { usePendingSendMoneroApproval } from "store/hooks";

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
  const pendingApprovals = usePendingSendMoneroApproval();
  const hasPendingApproval = pendingApprovals.length > 0;

  const [successDetails, setSuccessDetails] = useState<{
    address: string;
    amount: number;
  } | null>(null);

  const showSuccess = successDetails !== null;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="sm"
      fullWidth={!showSuccess}
      PaperProps={{
        sx: { borderRadius: 2 },
      }}
    >
      {!showSuccess && !hasPendingApproval && (
        <SendTransactionContent balance={balance} onClose={onClose} />
      )}
      {!showSuccess && hasPendingApproval && (
        <SendApprovalContent onClose={onClose} onSuccess={setSuccessDetails} />
      )}
      {showSuccess && (
        <SendSuccessContent onClose={onClose} successDetails={successDetails} />
      )}
    </Dialog>
  );
}
