import { Dialog } from "@mui/material";
import { usePendingSendMoneroApproval } from "store/hooks";
import SendTransactionContent from "./components/SendTransactionContent";
import SendApprovalContent from "./components/SendApprovalContent";

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

  // Force dialog to stay open if there's a pending approval
  const shouldShowDialog = open || hasPendingApproval;

  return (
    <Dialog
      open={shouldShowDialog}
      onClose={hasPendingApproval ? undefined : onClose}
      maxWidth="sm"
      fullWidth
      PaperProps={{
        sx: { borderRadius: 2 },
      }}
    >
      {hasPendingApproval ? (
        <SendApprovalContent />
      ) : (
        <SendTransactionContent balance={balance} onClose={onClose} />
      )}
    </Dialog>
  );
}
