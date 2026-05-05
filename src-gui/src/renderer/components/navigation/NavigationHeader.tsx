import { Box, List, Badge } from "@mui/material";
import HistoryOutlinedIcon from "@mui/icons-material/HistoryOutlined";
import SwapHorizOutlinedIcon from "@mui/icons-material/SwapHorizOutlined";
import LocalOfferOutlinedIcon from "@mui/icons-material/LocalOfferOutlined";
import FeedbackOutlinedIcon from "@mui/icons-material/FeedbackOutlined";
import RouteListItemIconButton from "./RouteListItemIconButton";
import UnfinishedSwapsBadge from "./UnfinishedSwapsCountBadge";
import {
  useHasOfferPhaseSwap,
  useHasSwapPhaseSwap,
  useTotalUnreadMessagesCount,
} from "store/hooks";
import SettingsIcon from "@mui/icons-material/Settings";
import BitcoinIcon from "../icons/BitcoinIcon";
import MoneroIcon from "../icons/MoneroIcon";

export default function NavigationHeader() {
  return (
    <Box>
      <List>
        <RouteListItemIconButton name="Wallet" route={["/monero-wallet", "/"]}>
          <MoneroIcon />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Wallet" route="/bitcoin-wallet">
          <BitcoinIcon />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Offers" route={["/offers"]}>
          <OffersIconWithBadge />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Swaps" route={["/swap"]}>
          <SwapIconWithBadge />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="History" route="/history">
          <UnfinishedSwapsBadge>
            <HistoryOutlinedIcon />
          </UnfinishedSwapsBadge>
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Feedback" route="/feedback">
          <FeedbackIconWithBadge />
        </RouteListItemIconButton>
        <RouteListItemIconButton name="Settings" route="/settings">
          <SettingsIcon />
        </RouteListItemIconButton>
      </List>
    </Box>
  );
}

function FeedbackIconWithBadge() {
  const totalUnreadCount = useTotalUnreadMessagesCount();

  return (
    <Badge
      badgeContent={totalUnreadCount}
      color="primary"
      overlap="rectangular"
      invisible={totalUnreadCount === 0}
    >
      <FeedbackOutlinedIcon />
    </Badge>
  );
}

function SwapIconWithBadge() {
  const hasSwapPhaseSwap = useHasSwapPhaseSwap();

  return (
    <Badge invisible={!hasSwapPhaseSwap} variant="dot" color="primary">
      <SwapHorizOutlinedIcon />
    </Badge>
  );
}

function OffersIconWithBadge() {
  const hasOfferPhaseSwap = useHasOfferPhaseSwap();

  return (
    <Badge invisible={!hasOfferPhaseSwap} variant="dot" color="primary">
      <LocalOfferOutlinedIcon />
    </Badge>
  );
}
