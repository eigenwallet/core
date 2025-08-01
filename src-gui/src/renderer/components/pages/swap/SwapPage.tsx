import { Box } from "@mui/material";
import ApiAlertsBox from "./ApiAlertsBox";
import SwapWidget from "./swap/SwapWidget";
import { useIsMobile } from "../../../../utils/useIsMobile";

export default function SwapPage() {
  const isMobile = useIsMobile();

  return (
    <Box
      sx={{
        display: "flex",
        width: "100%",
        flexDirection: "column",
        alignItems: "center",
        paddingBottom: isMobile ? 2 : 1,
        gap: isMobile ? 1.5 : 1,
        padding: isMobile ? 1 : 0,
      }}
    >
      <ApiAlertsBox />
      <SwapWidget />
    </Box>
  );
}
