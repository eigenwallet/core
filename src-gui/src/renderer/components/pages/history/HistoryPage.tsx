import { Box } from "@mui/material";
import SwapTxLockAlertsBox from "../../alert/SwapTxLockAlertsBox";
import HistoryTable from "./table/HistoryTable";
import { useIsMobile } from "../../../../utils/useIsMobile";

export default function HistoryPage() {
  const isMobile = useIsMobile();

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: isMobile ? 1.5 : 2,
        width: "100%",
        px: isMobile ? 0 : 0,
      }}
    >
      <SwapTxLockAlertsBox />
      <HistoryTable />
    </Box>
  );
}
