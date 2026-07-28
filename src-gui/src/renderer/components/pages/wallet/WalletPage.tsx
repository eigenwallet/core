import { Box } from "@mui/material";
import { useAppSelector } from "store/hooks";
import WalletOverview from "./components/WalletOverview";
import WalletActionButtons from "./components/WalletActionButtons";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { useState } from "react";
import WalletRecoveryPage from "./components/WalletRecoveryPage";
import { WalletRecoveryData } from "renderer/rpc";

export default function WalletPage() {
  const walletBalance = useAppSelector((state) => state.bitcoinWallet.balance);
  const bitcoinAddress = useAppSelector((state) => state.bitcoinWallet.address);
  const [walletRecovery, setWalletRecovery] =
    useState<WalletRecoveryData | null>(null);

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
      {walletRecovery === null ? (
        <>
          <WalletOverview balance={walletBalance} />
          {bitcoinAddress && (
            <ActionableMonospaceTextBox
              content={bitcoinAddress}
              displayCopyIcon={true}
            />
          )}
          <WalletActionButtons onShowBitcoinSeed={setWalletRecovery} />
        </>
      ) : (
        <WalletRecoveryPage
          walletRecovery={walletRecovery}
          highlightedWallet="bitcoin"
          onBack={() => setWalletRecovery(null)}
        />
      )}
    </Box>
  );
}
