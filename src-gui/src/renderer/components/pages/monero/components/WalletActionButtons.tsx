import {
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  TextField,
} from "@mui/material";
import {
  Send as SendIcon,
  Input as InputIcon,
  SwapHoriz as SwapIcon,
  Restore as RestoreIcon,
} from "@mui/icons-material";
import { useState } from "react";
import { sendMoneroTransaction, setMoneroRestoreHeight } from "renderer/rpc";
import SendTransactionModal from "../SendTransactionModal";
import { useNavigate } from "react-router-dom";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import DfxButton from "./DFXWidget";

interface WalletActionButtonsProps {
  balance: {
    unlocked_balance: string;
  };
}

function RestoreHeightDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [restoreHeight, setRestoreHeight] = useState(0);

  const handleRestoreHeight = async () => {
    await setMoneroRestoreHeight(restoreHeight);
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Restore Height</DialogTitle>
      <DialogContent>
        <TextField
          label="Restore Height"
          type="number"
          value={restoreHeight}
          onChange={(e) => setRestoreHeight(Number(e.target.value))}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleRestoreHeight}
          displayErrorSnackbar={true}
          variant="contained"
        >
          Restore
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}

export default function WalletActionButtons({
  balance,
}: WalletActionButtonsProps) {
  const navigate = useNavigate();
  const [sendDialogOpen, setSendDialogOpen] = useState(false);
  const [restoreHeightDialogOpen, setRestoreHeightDialogOpen] = useState(false);

  return (
    <>
      <RestoreHeightDialog
        open={restoreHeightDialogOpen}
        onClose={() => setRestoreHeightDialogOpen(false)}
      />
      <SendTransactionModal
        balance={balance}
        open={sendDialogOpen}
        onClose={() => setSendDialogOpen(false)}
      />
      <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1, mb: 2 }}>
        <Chip
          icon={<SendIcon />}
          label="Send"
          variant="button"
          clickable
          onClick={() => setSendDialogOpen(true)}
        />
        <Chip
          onClick={() => navigate("/swap")}
          icon={<SwapIcon />}
          label="Swap"
          variant="button"
          clickable
        />
        <Chip
          onClick={() => setRestoreHeightDialogOpen(true)}
          icon={<RestoreIcon />}
          label="Restore Height"
          variant="button"
          clickable
        />
        <DfxButton />
      </Box>
    </>
  );
}
