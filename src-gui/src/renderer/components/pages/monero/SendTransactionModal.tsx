import { Dialog } from "@mui/material";
import SendTransactionContent from "./components/SendTransactionContent";
import SendApprovalContent from "./components/SendApprovalContent";
import { useState } from "react";
import SendSuccessContent from "./components/SendSuccessContent";
import { usePendingSendMoneroApproval } from "store/hooks";
import { SendMoneroResponse } from "models/tauriModel";

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

  const [successResponse, setSuccessResponse] =
    useState<SendMoneroResponse | null>(null);

  const showSuccess = successResponse !== null;

  const handleClose = (event: unknown, reason: string) => {
    // We want the user to explicitly close the dialog.
    // We do not close the dialog upon a backdrop click.
    if (reason === "backdropClick") {
      return;
    }

    onClose();
    setSuccessResponse(null);
  };

  return (
    <Dialog
      open={open}
      onClose={handleClose}
      maxWidth="sm"
      fullWidth={!showSuccess}
      PaperProps={{
        sx: { borderRadius: 2 },
      }}
    >
      {!showSuccess && !hasPendingApproval && (
        <SendTransactionContent
          balance={balance}
          onClose={onClose}
          onSuccess={setSuccessResponse}
        />
      )}
      {!showSuccess && hasPendingApproval && (
        <SendApprovalContent onClose={onClose} />
      )}
      {showSuccess && (
        <SendSuccessContent
          onClose={handleClose}
          successDetails={successResponse}
        />
      )}
    </Dialog>
  );
}
