import {
  Box,
  IconButton,
  Tooltip,
  Dialog,
  DialogTitle,
  DialogContent,
} from "@mui/material";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import { generateBitcoinAddresses } from "renderer/rpc";
import WalletOverview from "./components/WalletOverview";
import WalletActionButtons from "./components/WalletActionButtons";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { Add as AddIcon } from "@mui/icons-material";
import { TransactionHistory } from "renderer/components/pages/monero/components";

export default function WalletPage() {
  const { balance, address, history } = useAppSelector(
    (state) => state.bitcoinWallet,
  );
  const [moreAddresses, setMoreAddresses] = useState<string[] | null>(null);

  return (
    <Box
      sx={{
        maxWidth: 800,
        mx: "auto",
        display: "flex",
        flexDirection: "column",
        gap: 2,
        pb: 2,
      }}
    >
      <Dialog open={!!moreAddresses} onClose={() => setMoreAddresses(null)}>
        <DialogTitle>Addresses</DialogTitle>
        <DialogContent sx={{ minWidth: "500px", minHeight: "300px" }}>
          {moreAddresses &&
            moreAddresses.map((a) => (
              <ActionableMonospaceTextBox
                key={a}
                content={a}
                displayCopyIcon={true}
              />
            ))}
        </DialogContent>
      </Dialog>
      <WalletOverview balance={balance} />
      {address && (
        <Box sx={{ display: "flex" }}>
          <Box sx={{ flexGrow: 1 }}>
            <ActionableMonospaceTextBox
              content={address}
              displayCopyIcon={true}
            />
          </Box>

          <PromiseInvokeButton
            sx={{ height: "100%", padding: "initial" }}
            tooltipTitle="More addresses"
            isIconButton={true}
            onInvoke={() => generateBitcoinAddresses(7)}
            onSuccess={setMoreAddresses}
          >
            <AddIcon />
          </PromiseInvokeButton>
        </Box>
      )}
      <WalletActionButtons />
      <TransactionHistory currency="bitcoin" transactions={history} />
    </Box>
  );
}
