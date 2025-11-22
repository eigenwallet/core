import { Dialog } from "@mui/material";
import SendTransactionContent from "./components/SendTransactionContent";
import SendApprovalContent from "./components/SendApprovalContent";
import { useState } from "react";
import SendSuccessContent from "./components/SendSuccessContent";
import { usePendingSendCurrencyApproval } from "store/hooks";
import { SendMoneroResponse, WithdrawBtcResponse } from "models/tauriModel";

interface SendTransactionModalProps {
  open: boolean;
  onClose: () => void;
  unlocked_balance: number;
  wallet: "monero" | "bitcoin";
}

export default function SendTransactionModal({
  open,
  onClose,
  unlocked_balance,
  wallet,
}: SendTransactionModalProps) {
  const pendingApprovals = usePendingSendCurrencyApproval(wallet);
  const hasPendingApproval = pendingApprovals.length > 0;

  const [successResponse, setSuccessResponse] = useState<
    SendMoneroResponse | WithdrawBtcResponse | null
  >(null);

  const showSuccess = successResponse !== null;

  const handleClose = () => {
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
          unlocked_balance={unlocked_balance}
          wallet={wallet}
          onClose={onClose}
          onSuccess={setSuccessResponse}
        />
      )}
      {!showSuccess && hasPendingApproval && (
        <SendApprovalContent
          onClose={onClose}
          pendingApprovals={pendingApprovals}
        />
      )}
      {showSuccess && (
        <SendSuccessContent
          onClose={onClose}
          successDetails={successResponse}
          wallet={wallet}
        />
      )}
    </Dialog>
  );
}
