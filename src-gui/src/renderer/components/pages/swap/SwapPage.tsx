import { Box } from "@mui/material";
import ApiAlertsBox from "./ApiAlertsBox";
import SwapWidget, { SwapWidgetMode } from "./swap/SwapWidget";
import AntiSpamInfoModal from "../../modal/anti-spam-info/AntiSpamInfoModal";

export default function SwapPage({ mode }: { mode: SwapWidgetMode }) {
  return (
    <Box
      sx={{
        display: "flex",
        width: "100%",
        flexDirection: "column",
        alignItems: "center",
        paddingBottom: 1,
        gap: 1,
      }}
    >
      <AntiSpamInfoModal />
      <ApiAlertsBox />
      <SwapWidget mode={mode} />
    </Box>
  );
}
