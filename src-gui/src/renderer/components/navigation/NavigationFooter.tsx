import { Box, Tooltip } from "@mui/material";
import { BackgroundProgressAlerts } from "../alert/DaemonStatusAlert";
import FundsLeftInWalletAlert from "../alert/FundsLeftInWalletAlert";
import UnfinishedSwapsAlert from "../alert/UnfinishedSwapsAlert";
import BackgroundRefundAlert from "../alert/BackgroundRefundAlert";
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
      <FundsLeftInWalletAlert />
      <UnfinishedSwapsAlert />
      <BackgroundRefundAlert />
      <BackgroundProgressAlerts />
      <ContactInfoBox />
    </Box>
  );
}
