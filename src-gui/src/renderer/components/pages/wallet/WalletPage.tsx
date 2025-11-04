import { Box } from "@mui/material";
import { useAppSelector } from "store/hooks";
import WalletOverview from "./components/WalletOverview";
import WalletActionButtons from "./components/WalletActionButtons";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";

export default function WalletPage() {
  const walletBalance = useAppSelector((state) => state.bitcoinWallet.balance);
  const bitcoinAddress = useAppSelector((state) => state.bitcoinWallet.address);

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
      <WalletOverview balance={walletBalance} />
      {bitcoinAddress && (
        <ActionableMonospaceTextBox
          content={bitcoinAddress}
          displayCopyIcon={true}
        />
      )}
      <WalletActionButtons />
    </Box>
  );
}
