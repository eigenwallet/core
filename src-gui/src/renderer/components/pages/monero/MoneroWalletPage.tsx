import { useEffect } from "react";
import { Box } from "@mui/material";
import { useAppSelector } from "store/hooks";
import {
  initializeMoneroWallet,
} from "renderer/rpc";
import {
  WalletOverview,
  TransactionHistory,
  WalletActionButtons,
} from "./components";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  const { mainAddress, balance, syncProgress, history, isRefreshing } =
    useAppSelector((state) => state.wallet.state);

  useEffect(() => {
    initializeMoneroWallet();
  }, []);

  if (mainAddress === null || balance === null || syncProgress === null) {
    return <div>Loading...</div>;
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
