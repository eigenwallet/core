import { Box } from "@mui/material";
import ApiAlertsBox from "./ApiAlertsBox";
import SwapWidget from "./swap/SwapWidget";

const swapPageSx = {
  display: "flex",
  width: "100%",
  flexDirection: "column",
  alignItems: "center",
  paddingBottom: 1,
  gap: 1,
};

export default function SwapPage() {
  return (
    <Box sx={swapPageSx}>
      <ApiAlertsBox />
      <SwapWidget />
    </Box>
  );
}
