import { CssBaseline } from "@mui/material";
import { ThemeProvider, StyledEngineProvider } from "@mui/material/styles";
import "@tauri-apps/plugin-shell";
import { useSettings } from "store/hooks";
import { Theme as ThemeEnum, themes } from "./theme";
import { useEffect } from "react";
import { setupBackgroundTasks } from "renderer/background";
import { useIsMobile } from "../../utils/useIsMobile";
import "@fontsource/roboto";
import { LocalizationProvider } from "@mui/x-date-pickers/LocalizationProvider";
import { AdapterDayjs } from "@mui/x-date-pickers/AdapterDayjs";
import DesktopApp from "renderer/layout/desktop/DesktopApp";
import MobileApp from "renderer/layout/mobile/MobileApp";

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
  const isMobile = useIsMobile();

  return (
    <StyledEngineProvider injectFirst>
      <ThemeProvider theme={currentTheme}>
        <LocalizationProvider dateAdapter={AdapterDayjs}>
          <CssBaseline />
          {isMobile ? <MobileApp /> : <DesktopApp />}
        </LocalizationProvider>
      </ThemeProvider>
    </StyledEngineProvider>
  );
}
