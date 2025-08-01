import GlobalSnackbarProvider from "renderer/components/snackbar/GlobalSnackbarProvider";
import FeedbackPage from "renderer/components/pages/feedback/FeedbackPage";
import IntroductionModal from "renderer/components/modal/introduction/IntroductionModal";
import MoneroWalletPage from "renderer/components/pages/monero/MoneroWalletPage";
import SeedSelectionDialog from "renderer/components/modal/seed-selection/SeedSelectionDialog";
import PasswordEntryDialog from "renderer/components/modal/password-entry/PasswordEntryDialog";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import Navigation, {
  drawerWidth,
} from "renderer/components/navigation/Navigation";
import SettingsPage from "renderer/components/pages/help/SettingsPage";
import HistoryPage from "renderer/components/pages/history/HistoryPage";
import SwapPage from "renderer/components/pages/swap/SwapPage";
import WalletPage from "renderer/components/pages/wallet/WalletPage";
import UpdaterDialog from "renderer/components/modal/updater/UpdaterDialog";
import { Box } from "@mui/material";

export default function DesktopApp() {
  return (
    <GlobalSnackbarProvider>
      <IntroductionModal />
      <SeedSelectionDialog />
      <PasswordEntryDialog />
      <Router>
        <Navigation />
        <InnerContent />
        <UpdaterDialog />
      </Router>
    </GlobalSnackbarProvider>
  );
}

function InnerContent() {
  return (
    <Box
      sx={{
        padding: 4,
        marginLeft: drawerWidth,
        paddingBottom: 4, // Account for bottom nav
        flex: 1,
      }}
    >
      <Routes>
        <Route path="/" element={<MoneroWalletPage />} />
        <Route path="/monero-wallet" element={<MoneroWalletPage />} />
        <Route path="/swap" element={<SwapPage />} />
        <Route path="/history" element={<HistoryPage />} />
        <Route path="/bitcoin-wallet" element={<WalletPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="/feedback" element={<FeedbackPage />} />
      </Routes>
    </Box>
  );
}
