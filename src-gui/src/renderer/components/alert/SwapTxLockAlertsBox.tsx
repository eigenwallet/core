import { Box } from "@mui/material";
import { useSwapInfosSortedByDate } from "store/hooks";
import SwapStatusAlert from "./SwapStatusAlert/SwapStatusAlert";

export default function SwapTxLockAlertsBox() {
  // We specifically choose ALL swaps here. SwapStatusAlert renders nothing for
  // swaps without a relevant timelock alert (no funds locked / already done).
  const swaps = useSwapInfosSortedByDate();

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      {swaps.map((swap) => (
        <SwapStatusAlert key={swap.swap_id} swap={swap} />
      ))}
    </Box>
  );
}
