import { BottomNavigation, BottomNavigationAction, Paper, Badge } from "@mui/material";
import { useLocation, useNavigate } from "react-router-dom";
import { useState, useEffect } from "react";
import MoneroIcon from "../icons/MoneroIcon";
import BitcoinIcon from "../icons/BitcoinIcon";
import SwapHorizOutlinedIcon from "@mui/icons-material/SwapHorizOutlined";
import HistoryOutlinedIcon from "@mui/icons-material/HistoryOutlined";
import SettingsIcon from "@mui/icons-material/Settings";
import FeedbackOutlinedIcon from "@mui/icons-material/FeedbackOutlined";
import { useIsSwapRunning, useTotalUnreadMessagesCount } from "store/hooks";
import UnfinishedSwapsBadge from "./UnfinishedSwapsCountBadge";

const routeToIndex: Record<string, number> = {
  '/monero-wallet': 0,
  '/': 0,
  '/bitcoin-wallet': 1, 
  '/swap': 2,
  '/history': 3,
  '/settings': 4,
  '/feedback': 5
};

export default function MobileBottomNavigation() {
  const location = useLocation();
  const navigate = useNavigate();
  const totalUnreadCount = useTotalUnreadMessagesCount();
  const isSwapRunning = useIsSwapRunning();
  
  const [value, setValue] = useState(routeToIndex[location.pathname] || 0);

  useEffect(() => {
    setValue(routeToIndex[location.pathname] || 0);
  }, [location.pathname]);

  const handleChange = (event: React.SyntheticEvent, newValue: number) => {
    setValue(newValue);
    const routes = ['/monero-wallet', '/bitcoin-wallet', '/swap', '/history', '/settings', '/feedback'];
    navigate(routes[newValue]);
  };

  return (
    <Paper 
      sx={{ 
        position: 'fixed', 
        bottom: 0, 
        left: 0, 
        right: 0, 
        zIndex: 1100,
        borderRadius: 0
      }} 
      elevation={3}
    >
      <BottomNavigation value={value} onChange={handleChange} showLabels sx={{ backgroundColor: "grey.900" }}>
        <BottomNavigationAction 
          label="XMR" 
          icon={<MoneroIcon />} 
        />
        <BottomNavigationAction 
          label="BTC" 
          icon={<BitcoinIcon />} 
        />
        <BottomNavigationAction 
          label="Swap" 
          icon={
            <Badge invisible={!isSwapRunning} variant="dot" color="primary">
              <SwapHorizOutlinedIcon />
            </Badge>
          } 
        />
        <BottomNavigationAction 
          label="History" 
          icon={
            <UnfinishedSwapsBadge>
              <HistoryOutlinedIcon />
            </UnfinishedSwapsBadge>
          } 
        />
        <BottomNavigationAction 
          label="Settings" 
          icon={<SettingsIcon />} 
        />
        <BottomNavigationAction 
          label="Support" 
          icon={
            <Badge
              badgeContent={totalUnreadCount}
              color="primary"
              invisible={totalUnreadCount === 0}
            >
              <FeedbackOutlinedIcon />
            </Badge>
          } 
        />
      </BottomNavigation>
    </Paper>
  );
}