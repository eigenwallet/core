import { useEffect } from "react";
import { Box, Typography } from "@mui/material";
import { useAppSelector } from "store/hooks";
import {
  updateMoneroSyncProgress,
  initializeMoneroWallet,
  sendMoneroTransaction,
  refreshMoneroWallet,
} from "renderer/rpc";
import {
  WalletOverview,
  SyncProgress,
  SendTransaction,
  TransactionHistory,
} from "./components";
import { GetMoneroBalanceResponse } from "models/tauriModel";

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  const { mainAddress, balance, syncProgress, history, isRefreshing } =
    useAppSelector((state) => state.wallet.state);

  // Auto-refresh sync progress every 5 seconds if not fully synced
  useEffect(() => {
    if (!syncProgress || syncProgress.progress_percentage >= 100) {
      return;
    }

    const interval = setInterval(() => {
      updateMoneroSyncProgress();
    }, 5000);

    return () => clearInterval(interval);
  }, [syncProgress]);

  useEffect(() => {
    initializeMoneroWallet();
  }, []);

  const handleSendTransaction = async (transactionData) => {
    await sendMoneroTransaction(transactionData);
  };

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

      <SendTransaction balance={balance} onSend={handleSendTransaction} />

      <TransactionHistory history={history} />
    </Box>
  );
}
