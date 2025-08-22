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
import { useIsMobile } from "../../../../utils/useIsMobile";

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  const { mainAddress, balance, history } = useAppSelector(
    (state) => state.wallet.state,
  );
  const isMobile = useIsMobile();

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
        maxWidth: isMobile ? "100%" : 800,
        mx: "auto",
        display: "flex",
        flexDirection: "column",
        gap: isMobile ? 1.5 : 2,
        pb: isMobile ? 1 : 2,
        px: isMobile ? 0 : 0,
      }}
    >
      <WalletOverview balance={balance} />
      <ActionableMonospaceTextBox
        content={mainAddress}
        displayCopyIcon={true}
      />
      <WalletActionButtons balance={balance} />
      <TransactionHistory history={history} />
    </Box>
  );
}
