import { Box } from "@mui/material";
import DonateInfoBox from "./DonateInfoBox";
import DaemonControlBox from "./DaemonControlBox";
import SettingsBox from "./SettingsBox";
import DiscoveryBox from "./DiscoveryBox";
import MoneroPoolHealthBox from "./MoneroPoolHealthBox";
import { useLocation } from "react-router-dom";
import { useEffect } from "react";

export default function SettingsPage() {
  const location = useLocation();

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
        gap: 2,
        flexDirection: "column",
        paddingBottom: 2,
      }}
    >
      <DonateInfoBox />
      <SettingsBox />
      <DaemonControlBox />
      <MoneroPoolHealthBox />
      <DiscoveryBox />
    </Box>
  );
}
