import { Box, IconButton, Typography } from "@mui/material";
import SettingsBox from "renderer/components/pages/help/SettingsBox";
import { useNavigate } from "react-router-dom";
import { ChevronLeft } from "@mui/icons-material";
import DonateInfoBox from "renderer/components/pages/help/DonateInfoBox";
import ExportDataBox from "renderer/components/pages/help/ExportDataBox";
import DiscoveryBox from "renderer/components/pages/help/DiscoveryBox";
import MoneroPoolHealthBox from "renderer/components/pages/help/MoneroPoolHealthBox";
import DaemonControlBox from "renderer/components/pages/help/DaemonControlBox";

export default function SettingsPage() {
  const navigate = useNavigate();
  return (
    <Box>
      <Box sx={{ px: 2, pt: 3, display: "flex", alignItems: "center", gap: 1, position: "sticky", top: 0, backgroundColor: "background.paper", zIndex: 1 }}>
        <IconButton onClick={() => navigate("/", { viewTransition: true })}>
          <ChevronLeft />
        </IconButton>
        <Typography variant="h5">Settings</Typography>
      </Box>
      <Box sx={{ p: 2, display: "flex", flexDirection: "column", gap: 2 }}>
        <SettingsBox />
        <DiscoveryBox />
        <MoneroPoolHealthBox />
        <ExportDataBox />
        <DaemonControlBox />
        <DonateInfoBox />
      </Box>
    </Box>
  );
}
