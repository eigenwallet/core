import { Box } from "@mui/material";
import { useSwapInfosSortedByDate } from "store/hooks";
import SwapStatusAlert from "./SwapStatusAlert/SwapStatusAlert";

export default function SwapTxLockAlertsBox() {
  // All swaps; SwapStatusAlert renders nothing for those without a timelock alert.
  const swaps = useSwapInfosSortedByDate();

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      {swaps.map((swap) => (
        <SwapStatusAlert key={swap.swap_id} swap={swap} />
      ))}
    </Box>
  );
}
