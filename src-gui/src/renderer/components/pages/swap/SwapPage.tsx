import { Box } from "@mui/material";
import { useEffect, useRef } from "react";
import { useLocation } from "react-router-dom";
import { haveFundsBeenLocked } from "models/tauriModelExt";
import { suspendCurrentSwap } from "renderer/rpc";
import { useAppSelector } from "store/hooks";
import ApiAlertsBox from "./ApiAlertsBox";
import SwapWidget from "./swap/SwapWidget";

export default function SwapPage() {
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
      <ApiAlertsBox />
      <SwapWidget />
    </Box>
  );
}
