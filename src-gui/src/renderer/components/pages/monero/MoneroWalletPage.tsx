import { useEffect } from "react";
import { Box, Typography } from "@mui/material";
import { useAppSelector } from "store/hooks";
import {
  updateMoneroSyncProgress,
  initializeMoneroWallet,
  sendMoneroTransaction,
} from "renderer/rpc";
import {
  WalletOverview,
  SendTransaction,
  TransactionHistory,
  WalletActionButtons,
} from "./components";

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  const { mainAddress, balance, syncProgress, history, isRefreshing } =
    useAppSelector((state) => state.wallet.state);

  useEffect(() => {
    initializeMoneroWallet();
  }, []);

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
      <Typography variant="h4">Wallet</Typography>
      <WalletOverview balance={balance} syncProgress={syncProgress} />
      <WalletActionButtons balance={balance} />

      <TransactionHistory history={history} />
    </Box>
  );
}
