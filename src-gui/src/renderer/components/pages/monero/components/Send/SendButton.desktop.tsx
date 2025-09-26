import { Box, Chip } from "@mui/material";
import { useState } from "react";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import { SendMoneroResponse } from "models/tauriModel";
import { usePendingSendMoneroApproval } from "store/hooks";
import SendTransactionContent from "./SendTransactionContent";
import SendApprovalContent from "./SendApprovalContent";
import SendSuccessContent from "./SendSuccessContent";
import MobileDialog from "renderer/components/modal/MobileDialog";

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
  const pendingApprovals = usePendingSendMoneroApproval();
  const hasPendingApproval = pendingApprovals.length > 0;

  const [successResponse, setSuccessResponse] =
    useState<SendMoneroResponse | null>(null);

  const showSuccess = successResponse !== null;

  const handleClose = () => {
    setOpen(false);
    setSuccessResponse(null);
  };

  return (
    <>
      <Chip
        icon={<ArrowUpwardIcon />}
        label="Send"
        variant="button"
        clickable
        onClick={() => setOpen(true)}
        disabled={disabled}
      />
      <MobileDialog
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
            onClose={handleClose}
            onSuccess={setSuccessResponse}
          />
        )}
        {!showSuccess && hasPendingApproval && (
          <SendApprovalContent onClose={handleClose} />
        )}
        {showSuccess && (
          <SendSuccessContent
            onClose={handleClose}
            successDetails={successResponse}
          />
        )}
      </MobileDialog>
    </>
  );
}
