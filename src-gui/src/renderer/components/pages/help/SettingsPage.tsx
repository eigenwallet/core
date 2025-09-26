import { Box } from "@mui/material";
import DonateInfoBox from "./DonateInfoBox";
import DaemonControlBox from "./DaemonControlBox";
import SettingsBox from "./SettingsBox";
import ExportDataBox from "./ExportDataBox";
import DiscoveryBox from "./DiscoveryBox";
import MoneroPoolHealthBox from "./MoneroPoolHealthBox";
import { useLocation } from "react-router-dom";
import { useEffect } from "react";
import { useIsMobile } from "../../../../utils/useIsMobile";

export default function SettingsPage() {
  const location = useLocation();
  const isMobile = useIsMobile();

  useEffect(() => {
    if (location.hash) {
      const element = document.getElementById(location.hash.slice(1));
      element?.scrollIntoView({ behavior: "smooth" });
    }
  }, [location]);

  return (
    <Box
      sx={{
        display: "flex",
        gap: isMobile ? 1.5 : 2,
        flexDirection: "column",
        paddingBottom: isMobile ? 1 : 2,
        maxWidth: isMobile ? "100%" : "none",
      }}
    >
      <SettingsBox />
      <DiscoveryBox />
      <MoneroPoolHealthBox />
      <ExportDataBox />
      <DaemonControlBox />
      <DonateInfoBox />
    </Box>
  );
}
