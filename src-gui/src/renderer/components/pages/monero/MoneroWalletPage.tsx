import { useEffect } from "react";
import { Box } from "@mui/material";
import { useAppSelector } from "store/hooks";
import { initializeMoneroWallet } from "renderer/rpc";
import {
  WalletOverview,
  TransactionHistory,
  WalletActionButtons,
  WalletSwitcher,
} from "./components";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import WalletPageLoadingState from "./components/WalletPageLoadingState";

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  const { mainAddress, balance, syncProgress, history } = useAppSelector(
    (state) => state.wallet.state,
  );

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
        position: "relative",
      }}
    >
      <Box
        sx={{
          position: "absolute",
          top: 0,
          right: 24,
          zIndex: 1,
        }}
      >
        <WalletSwitcher />
      </Box>
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
