import { Box, Chip, Drawer } from "@mui/material";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import { useState } from "react";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import { SendMoneroResponse } from "models/tauriModel";
import { usePendingSendMoneroApproval } from "store/hooks";
import SendTransactionContent from "renderer/components/pages/monero/components/SendTransactionContent";
import SendApprovalContent from "renderer/components/pages/monero/components/SendApprovalContent";
import SendSuccessContent from "renderer/components/pages/monero/components/SendSuccessContent";
import MobileDialog from "renderer/components/modal/MobileDialog";
import { useIsMobile } from "../../../../utils/useIsMobile";

export default function SendButton({
  balance,
}: {
  balance: {
    unlocked_balance: string;
  };
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

  const content = (
    <>
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
    </>
  );

  return useIsMobile() ? (
    <SendTransactionMobile
      open={open}
      onOpen={() => setOpen(true)}
      onClose={handleClose}
    >
      {content}
    </SendTransactionMobile>
  ) : (
    <SendTransactionDesktop
      open={open}
      onOpen={() => setOpen(true)}
      onClose={handleClose}
      showSuccess={showSuccess}
    >
      {content}
    </SendTransactionDesktop>
  );
}

function SendTransactionDesktop({
  children,
  open,
  onOpen,
  onClose,
  showSuccess,
}: {
  children: React.ReactNode;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
  showSuccess: boolean;
}) {
  return (
    <>
      <Chip
        icon={<ArrowUpwardIcon />}
        label="Send"
        variant="button"
        clickable
        onClick={onOpen}
      />
      <MobileDialog
        open={open}
        onClose={onClose}
        maxWidth="sm"
        fullWidth={!showSuccess}
        PaperProps={{
          sx: { borderRadius: 2 },
        }}
      >
        {children}
      </MobileDialog>
    </>
  );
}

function SendTransactionMobile({
  children,
  open,
  onOpen,
  onClose,
}: {
  children: React.ReactNode;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <TextIconButton label="Send" onClick={onOpen}>
        <ArrowUpwardIcon />
      </TextIconButton>
      <Drawer open={open} onClose={onClose} anchor="bottom">
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
          {children}
        </Box>
      </Drawer>
    </>
  );
}
