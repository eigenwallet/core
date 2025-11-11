import { Box, CssBaseline } from "@mui/material";
import {
  ThemeProvider,
  Theme,
  StyledEngineProvider,
} from "@mui/material/styles";
import "@tauri-apps/plugin-shell";
import { Route, MemoryRouter as Router, Routes } from "react-router-dom";
import Navigation, { drawerWidth } from "./navigation/Navigation";
import SettingsPage from "./pages/help/SettingsPage";
import HistoryPage from "./pages/history/HistoryPage";
import SwapPage from "./pages/swap/SwapPage";
import WalletPage from "./pages/wallet/WalletPage";
import GlobalSnackbarProvider from "./snackbar/GlobalSnackbarProvider";
import UpdaterDialog from "./modal/updater/UpdaterDialog";
import { useSettings } from "store/hooks";
import { Theme as ThemeEnum, themes } from "./theme";
import { useEffect } from "react";
import { setupBackgroundTasks } from "renderer/background";
import "@fontsource/roboto";
import FeedbackPage from "./pages/feedback/FeedbackPage";
import IntroductionModal from "./modal/introduction/IntroductionModal";
import MoneroWalletPage from "./pages/monero/MoneroWalletPage";
import SeedSelectionDialog from "./modal/seed-selection/SeedSelectionDialog";
import { LocalizationProvider } from "@mui/x-date-pickers/LocalizationProvider";
import { AdapterDayjs } from "@mui/x-date-pickers/AdapterDayjs";
import PasswordEntryDialog from "./modal/password-entry/PasswordEntryDialog";
import ContextErrorDialog from "./modal/context-error/ContextErrorDialog";
import ErrorBoundary from "./other/ErrorBoundary";

declare module "@mui/material/styles" {
  interface Theme {
    // Add your custom theme properties here if needed
  }
  interface ThemeOptions {
    // Add your custom theme options here if needed
  }
}

export default function App() {
  useEffect(() => {
    setupBackgroundTasks();
  }, []);

  const theme = useSettings((s) => s.theme);
  const currentTheme = themes[theme] || themes[ThemeEnum.Dark];

  console.log("Current theme:", { theme, currentTheme });

  return (
    <ErrorBoundary>
      <StyledEngineProvider injectFirst>
        <ThemeProvider theme={currentTheme}>
          <LocalizationProvider dateAdapter={AdapterDayjs}>
            <CssBaseline />
            <GlobalSnackbarProvider>
              <IntroductionModal />
              <SeedSelectionDialog />
              <PasswordEntryDialog />
              <ContextErrorDialog />
              <Router>
                <Navigation />
                <InnerContent />
                <UpdaterDialog />
              </Router>
            </GlobalSnackbarProvider>
          </LocalizationProvider>
        </ThemeProvider>
      </StyledEngineProvider>
    </ErrorBoundary>
  );
}

function InnerContent() {
  return (
    <Box
      sx={{
        padding: 4,
        marginLeft: drawerWidth,
        maxHeight: `100vh`,
        flex: 1,
      }}
    >
      <Routes>
        <Route
          path="/"
          element={
            <ErrorBoundary>
              <MoneroWalletPage />
            </ErrorBoundary>
          }
        />
        <Route
          path="/monero-wallet"
          element={
            <ErrorBoundary>
              <MoneroWalletPage />
            </ErrorBoundary>
          }
        />
        <Route
          path="/swap"
          element={
            <ErrorBoundary>
              <SwapPage />
            </ErrorBoundary>
          }
        />
        <Route
          path="/history"
          element={
            <ErrorBoundary>
              <HistoryPage />
            </ErrorBoundary>
          }
        />
        <Route
          path="/bitcoin-wallet"
          element={
            <ErrorBoundary>
              <WalletPage />
            </ErrorBoundary>
          }
        />
        <Route
          path="/settings"
          element={
            <ErrorBoundary>
              <SettingsPage />
            </ErrorBoundary>
          }
        />
        <Route
          path="/feedback"
          element={
            <ErrorBoundary>
              <FeedbackPage />
            </ErrorBoundary>
          }
        />
      </Routes>
    </Box>
  );
}
