import { Navigation } from "@mui/icons-material";
import { Box } from "@mui/material";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import IntroductionModal from "renderer/components/modal/introduction/IntroductionModal";
import PasswordEntryDialog from "renderer/components/modal/password-entry/PasswordEntryDialog";
import SeedSelectionDialog from "renderer/components/modal/seed-selection/SeedSelectionDialog";
import UpdaterDialog from "renderer/components/modal/updater/UpdaterDialog";
import { drawerWidth } from "renderer/components/navigation/Navigation";
import HomePage from "./pages/HomePage";

import GlobalSnackbarProvider from "renderer/components/snackbar/GlobalSnackbarProvider";

export default function MobileApp() {
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
          padding: 4,
          marginLeft: drawerWidth,
          paddingBottom: 4, // Account for bottom nav
          flex: 1,
        }}
      >
        <Routes>
          <Route path="/" element={<HomePage />} />
          {/* <Route path="/history" element={<HistoryPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/feedback" element={<FeedbackPage />} /> */}
        </Routes>
      </Box>
    );
  }