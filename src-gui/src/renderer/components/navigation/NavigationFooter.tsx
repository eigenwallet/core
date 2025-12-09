import { Box, Tooltip } from "@mui/material";
import { BackgroundProgressAlerts } from "../alert/DaemonStatusAlert";
import UnfinishedSwapsAlert from "../alert/UnfinishedSwapsAlert";
import ContactInfoBox from "../other/ContactInfoBox";

export default function NavigationFooter() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        padding: 1,
        gap: 1,
      }}
    >
      <UnfinishedSwapsAlert />
      <BackgroundProgressAlerts />
      <ContactInfoBox />
    </Box>
  );
}
