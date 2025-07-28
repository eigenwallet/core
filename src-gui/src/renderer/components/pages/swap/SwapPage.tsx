import { Box } from "@mui/material";
import { useEffect, useRef } from "react";
import { useLocation } from "react-router-dom";
import { haveFundsBeenLocked } from "models/tauriModelExt";
import { suspendCurrentSwap } from "renderer/rpc";
import { useAppSelector } from "store/hooks";
import ApiAlertsBox from "./ApiAlertsBox";
import SwapWidget from "./swap/SwapWidget";

export default function SwapPage() {
  const location = useLocation();
  const swap = useAppSelector((state) => state.swap);
  const suspensionTimerRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    const isOnSwapPage = location.pathname === "/swap";

    // Clear any existing timer when returning to swap page
    if (isOnSwapPage && suspensionTimerRef.current) {
      clearTimeout(suspensionTimerRef.current);
      suspensionTimerRef.current = null;
    }

    // Start suspension timer when leaving swap page
    if (!isOnSwapPage && swap.state?.curr) {
      // Only start timer if funds haven't been locked yet
      const fundsLocked = haveFundsBeenLocked(swap.state.curr);
      
      if (!fundsLocked) {
        suspensionTimerRef.current = setTimeout(async () => {
          try {
            await suspendCurrentSwap();
            console.log("Swap suspended due to inactivity (left swap page for 10+ seconds)");
          } catch (error) {
            console.error("Failed to suspend swap:", error);
          }
        }, 10000); // 10 seconds
      }
    }

    // Cleanup function
    return () => {
      if (suspensionTimerRef.current) {
        clearTimeout(suspensionTimerRef.current);
        suspensionTimerRef.current = null;
      }
    };
  }, [location.pathname, swap.state?.curr]);

  // Handle page visibility changes (when user switches tabs/windows)
  useEffect(() => {
    const handleVisibilityChange = () => {
      const isOnSwapPage = location.pathname === "/swap";
      
      if (document.hidden && isOnSwapPage && swap.state?.curr) {
        // Page became hidden while on swap page
        const fundsLocked = haveFundsBeenLocked(swap.state.curr);
        
        if (!fundsLocked) {
          suspensionTimerRef.current = setTimeout(async () => {
            try {
              await suspendCurrentSwap();
              console.log("Swap suspended due to inactivity (page hidden for 10+ seconds)");
            } catch (error) {
              console.error("Failed to suspend swap:", error);
            }
          }, 10000); // 10 seconds
        }
      } else if (!document.hidden && suspensionTimerRef.current) {
        // Page became visible again, cancel suspension
        clearTimeout(suspensionTimerRef.current);
        suspensionTimerRef.current = null;
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [location.pathname, swap.state?.curr]);

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
