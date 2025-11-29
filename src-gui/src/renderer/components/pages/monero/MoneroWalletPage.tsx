import { useEffect } from "react";
import { Box } from "@mui/material";
import { useAppSelector } from "store/hooks";
import { initializeMoneroWallet } from "renderer/rpc";
import {
  WalletOverview,
  TransactionHistory,
  WalletActionButtons,
} from "./components";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import WalletPageLoadingState from "./components/WalletPageLoadingState";

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  // Use separate selectors to prevent unnecessary re-renders
  // When syncProgress updates frequently during syncing, we don't want to
  // re-render components that only depend on history, balance, or mainAddress
  const mainAddress = useAppSelector((state) => state.wallet.state.mainAddress);
  const balance = useAppSelector((state) => state.wallet.state.balance);
  const syncProgress = useAppSelector(
    (state) => state.wallet.state.syncProgress
  );
  const history = useAppSelector((state) => state.wallet.state.history);

  useEffect(() => {
    initializeMoneroWallet();
  }, []);

  const isLoading = balance === null;

  if (isLoading) {
    return <WalletPageLoadingState />;
  }

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
      <WalletOverview balance={balance} syncProgress={syncProgress} />
      <ActionableMonospaceTextBox
        content={mainAddress}
        displayCopyIcon={true}
      />
      <WalletActionButtons balance={balance} />
      <TransactionHistory history={history} />
    </Box>
  );
}
