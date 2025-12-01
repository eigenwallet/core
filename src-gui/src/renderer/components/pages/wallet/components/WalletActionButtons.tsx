import { Box, Chip } from "@mui/material";
import { Send as SendIcon } from "@mui/icons-material";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import WalletDescriptorButton from "./WalletDescriptorButton";
import SendTransactionModal from "../../monero/SendTransactionModal";

export default function WalletActionButtons() {
  const [sendDialogOpen, setSendDialogOpen] = useState(false);
  const balance = useAppSelector((state) => state.bitcoinWallet.balance);

  return (
    <>
      <SendTransactionModal
        wallet="bitcoin"
        unlocked_balance={balance!}
        open={sendDialogOpen}
        onClose={() => setSendDialogOpen(false)}
      />
      <Box sx={{ display: "flex", justifyContent: "space-between" }}>
        <Box
          sx={{
            display: "flex",
            flexWrap: "wrap",
            gap: 1,
            alignItems: "center",
          }}
        >
          <Chip
            icon={<SendIcon />}
            label="Send"
            variant="button"
            clickable
            onClick={() => setSendDialogOpen(true)}
            disabled={balance === null || balance <= 0}
          />
          <WalletDescriptorButton />
        </Box>
      </Box>
    </>
  );
}
