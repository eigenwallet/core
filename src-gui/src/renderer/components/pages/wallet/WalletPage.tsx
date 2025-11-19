import { Box } from "@mui/material";
import { useAppSelector } from "store/hooks";
import WalletOverview from "./components/WalletOverview";
import WalletActionButtons from "./components/WalletActionButtons";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { TransactionHistory } from "renderer/components/pages/monero/components";

export default function WalletPage() {
  const { balance, address, history } = useAppSelector(
    (state) => state.bitcoinWallet,
  );

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
      <WalletOverview balance={balance} />
      {address && (
        <ActionableMonospaceTextBox content={address} displayCopyIcon={true} />
      )}
      <WalletActionButtons />
      <TransactionHistory currency="bitcoin" transactions={history} />
    </Box>
  );
}
