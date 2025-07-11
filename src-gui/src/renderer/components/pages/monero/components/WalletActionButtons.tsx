import { Box, Chip, Dialog } from "@mui/material";
import {
  Send as SendIcon,
  Input as InputIcon,
  SwapHoriz as SwapIcon,
} from "@mui/icons-material";
import SendTransaction from "./SendTransaction";
import { useState } from "react";
import { sendMoneroTransaction } from "renderer/rpc";
import SendTransactionModal from "../SendTransactionModal";
import { useNavigate } from "react-router-dom";

interface WalletActionButtonsProps {
  balance: {
    unlocked_balance: string;
  };
}

export default function WalletActionButtons({
  balance,
}: WalletActionButtonsProps) {
  const navigate = useNavigate();
  const [sendDialogOpen, setSendDialogOpen] = useState(false);

  return (
    <>
      <SendTransactionModal
        balance={balance}
        open={sendDialogOpen}
        onClose={() => setSendDialogOpen(false)}
      />
      <Box sx={{ display: "flex", gap: 1, mb: 2 }}>
        <Chip
          icon={<SendIcon />}
          label="Send"
          variant="button"
          clickable
          onClick={() => setSendDialogOpen(true)}
        />
        <Chip icon={<InputIcon />} label="Receive" variant="button" clickable />
        <Chip
          onClick={() => navigate("/swap")}
          icon={<SwapIcon />}
          label="Swap"
          variant="button"
          clickable
        />
      </Box>
    </>
  );
}
