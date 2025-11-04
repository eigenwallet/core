import { Box, Chip } from "@mui/material";
import { Send as SendIcon } from "@mui/icons-material";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import WithdrawDialog from "../../../modal/wallet/WithdrawDialog";
import WalletDescriptorButton from "./WalletDescriptorButton";

export default function WalletActionButtons() {
  const [showDialog, setShowDialog] = useState(false);
  const balance = useAppSelector((state) => state.bitcoinWallet.balance);

  return (
    <>
      <WithdrawDialog open={showDialog} onClose={() => setShowDialog(false)} />
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
            label="Sweep"
            variant="button"
            clickable
            onClick={() => setShowDialog(true)}
            disabled={balance === null || balance <= 0}
          />
          <WalletDescriptorButton />
        </Box>
      </Box>
    </>
  );
}
