import { Navigation } from "@mui/icons-material";
import { Box } from "@mui/material";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import IntroductionModal from "renderer/components/modal/introduction/IntroductionModal";
import PasswordEntryDialog from "renderer/components/modal/password-entry/PasswordEntryDialog";
import SeedSelectionDialog from "renderer/components/modal/seed-selection/SeedSelectionDialog";
import UpdaterDialog from "renderer/components/modal/updater/UpdaterDialog";
import HomePage from "./pages/HomePage";

import GlobalSnackbarProvider from "renderer/components/snackbar/GlobalSnackbarProvider";
import SettingsPage from "./pages/SettingsPage";
import FeedbackPage from "./pages/FeedbackPage";
import TransactionsPage from "./pages/HistoryPage";

export default function App() {
  return (
    <GlobalSnackbarProvider>
      <IntroductionModal />
      <SeedSelectionDialog />
      <PasswordEntryDialog />
      <Router>
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
        flex: 1,
      }}
    >
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="/feedback" element={<FeedbackPage />} />
        <Route path="/transactions" element={<TransactionsPage />} />
      </Routes>
    </Box>
  );
}
