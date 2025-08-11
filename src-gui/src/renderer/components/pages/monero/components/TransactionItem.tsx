import {
  Box,
  Chip,
  IconButton,
  Menu,
  MenuItem,
  Typography,
} from "@mui/material";
import { TransactionDirection, TransactionInfo } from "models/tauriModel";
import {
  CallReceived as IncomingIcon,
  MoreVert as MoreVertIcon,
} from "@mui/icons-material";
import { CallMade as OutgoingIcon } from "@mui/icons-material";
import {
  FiatPiconeroAmount,
  PiconeroAmount,
} from "renderer/components/other/Units";
import ConfirmationsBadge from "./ConfirmationsBadge";
import MoneroIcon from "renderer/components/icons/MoneroIcon";
import { getMoneroTxExplorerUrl } from "utils/conversionUtils";
import { isTestnet } from "store/config";
import { open } from "@tauri-apps/plugin-shell";
import dayjs from "dayjs";
import { useState } from "react";
import { useIsMobile } from "utils/useIsMobile";

interface TransactionItemProps {
  transaction: TransactionInfo;
  onClick?: () => void;
}

// // Custom Monero Icon SVG component
// function MoneroIcon({ size = 12 }: { size?: number }) {
//   return (
//     <svg
//       width={size}
//       height={size}
//       viewBox="0 0 24 24"
//       fill="currentColor"
//       xmlns="http://www.w3.org/2000/svg"
//     >
//       <path d="M12 0C5.373 0 0 5.373 0 12s5.373 12 12 12 12-5.373 12-12S18.627 0 12 0zm7.5 15h-3V9.5l-4.5 4.5-4.5-4.5V15h-3V8h3l4.5 4.5L16.5 8h3v7z"/>
//     </svg>
//   );
// }

// Mobile transaction layout component
function MobileTransactionLayout({ 
  transaction, 
  onClick,
  isIncoming,
  displayDate,
  amountStyles 
}: {
  transaction: TransactionInfo;
  onClick?: () => void;
  isIncoming: boolean;
  displayDate: string;
  amountStyles: any;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
        p: 1,
        cursor: onClick ? "pointer" : "default",
        "&:hover": onClick ? {
          backgroundColor: "action.hover",
          borderRadius: 1,
        } : {},
      }}
      onClick={onClick}
    >
      {/* Left Section - Icon with Monero badge */}
      <Box
        sx={{
          position: "relative",
          mr: 2,
        }}
      >
        {/* Main transaction icon */}
        <Box
          sx={{
            width: 48,
            height: 48,
            borderRadius: "50%",
            backgroundColor: isIncoming ? "success.main" : "error.main",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "white",
          }}
        >
          {isIncoming ? <IncomingIcon fontSize="medium" /> : <OutgoingIcon fontSize="medium" />}
        </Box>
        
        {/* Monero badge in bottom right corner */}
        <Box
          sx={{
            position: "absolute",
            bottom: -3,
            right: -3,
            width: 25,
            height: 25,
            borderRadius: "50%",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            border: "2px solid",
            borderColor: "background.paper",
            backgroundColor: "background.paper",
          }}
        >
          <MoneroIcon fontSize="small" sx={{ width: "100%", height: "100%", color: "warning.main" }} />
        </Box>
      </Box>

      {/* Middle Section - Transaction details */}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        {/* Transaction type */}
        <Typography
          variant="body1"
          sx={{
            fontWeight: 500,
            fontSize: "1rem",
            color: "text.primary",
            lineHeight: 1.2,
          }}
        >
          {isIncoming ? "Received Monero" : "Sent Monero"}
        </Typography>
        
        {/* Date */}
        <Typography
          variant="caption"
          sx={{
            color: "text.secondary",
            fontSize: "0.875rem",
            lineHeight: 1.2,
          }}
        >
          {displayDate}
        </Typography>
      </Box>

      {/* Right Section - Amount display */}
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-end",
          minWidth: 0,
        }}
      >
        {/* Cryptocurrency amount */}
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            gap: 0.5,
          }}
        >
          <Typography
            variant="h6"
            sx={{
              opacity: !isIncoming ? 1 : 0,
              fontWeight: "bold",
              ...amountStyles,
              fontSize: "1rem",
            }}
          >
            {!isIncoming ? "−" : ""}
          </Typography>
          <Typography
            variant="h6"
            sx={{ 
              fontWeight: "bold", 
              ...amountStyles,
              fontSize: "1rem",
            }}
          >
            <PiconeroAmount
              amount={transaction.amount}
              labelStyles={{ fontSize: 14, ml: -0.3 }}
              disableTooltip
            />
          </Typography>
        </Box>
        
        {/* Fiat equivalent */}
        <Typography 
          variant="caption" 
          sx={{ 
            color: "text.secondary",
            fontSize: "0.875rem",
          }}
        >
          <FiatPiconeroAmount amount={transaction.amount} />
        </Typography>
      </Box>
    </Box>
  );
}

// Desktop transaction layout component
function DesktopTransactionLayout({ 
  transaction,
  isIncoming,
  displayDate,
  amountStyles,
  menuAnchorEl,
  setMenuAnchorEl,
  menuOpen 
}: {
  transaction: TransactionInfo;
  isIncoming: boolean;
  displayDate: string;
  amountStyles: any;
  menuAnchorEl: HTMLElement | null;
  setMenuAnchorEl: (element: HTMLElement | null) => void;
  menuOpen: boolean;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "space-between",
      }}
    >
      <Box
        sx={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          gap: 1,
        }}
      >
        <Box
          sx={{
            p: 0.5,
            backgroundColor: "grey.800",
            borderRadius: "100%",
            height: 40,
            aspectRatio: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          {isIncoming ? <IncomingIcon /> : <OutgoingIcon />}
        </Box>
        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: "min-content max-content",
            columnGap: 0.5,
          }}
        >
          <Typography
            variant="h6"
            sx={{
              opacity: !isIncoming ? 1 : 0,
              gridArea: "1 / 1",
              fontWeight: "bold",
              ...amountStyles,
            }}
          >
            ‐
          </Typography>
          <Typography
            variant="h6"
            sx={{ gridArea: "1 / 2", fontWeight: "bold", ...amountStyles }}
          >
            <PiconeroAmount
              amount={transaction.amount}
              labelStyles={{ fontSize: 14, ml: -0.3 }}
              disableTooltip
            />
          </Typography>
          <Box
            sx={{
              gridArea: "2 / 2",
              display: "flex",
              flexDirection: "row",
              gap: 1,
            }}
          >
            <Typography variant="caption">
              <FiatPiconeroAmount amount={transaction.amount} />
            </Typography>
          </Box>
        </Box>
      </Box>
      <Box
        sx={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          gap: 1,
        }}
      >
        <Typography
          variant="body1"
          color="text.secondary"
          sx={{ fontSize: 14 }}
        >
          {displayDate}
        </Typography>
        <ConfirmationsBadge confirmations={transaction.confirmations} />
        <IconButton
          onClick={(event) => {
            setMenuAnchorEl(event.currentTarget);
          }}
        >
          <MoreVertIcon />
        </IconButton>
        <Menu
          anchorEl={menuAnchorEl}
          open={menuOpen}
          onClose={() => setMenuAnchorEl(null)}
        >
          <MenuItem
            onClick={() => {
              navigator.clipboard.writeText(transaction.tx_hash);
              setMenuAnchorEl(null);
            }}
          >
            <Typography>Copy Transaction ID</Typography>
          </MenuItem>
          <MenuItem
            onClick={() => {
              open(getMoneroTxExplorerUrl(transaction.tx_hash, isTestnet()));
              setMenuAnchorEl(null);
            }}
          >
            <Typography>View on Explorer</Typography>
          </MenuItem>
        </Menu>
      </Box>
    </Box>
  );
}

export default function TransactionItem({ transaction, onClick }: TransactionItemProps) {
  const isIncoming = transaction.direction === TransactionDirection.In;
  const isMobile = useIsMobile();
  
  // Different date formats for mobile vs desktop
  const displayDate = isMobile 
    ? dayjs(transaction.timestamp * 1000).format("MMM DD, HH:mm")
    : dayjs(transaction.timestamp * 1000).format("MMM DD YYYY, HH:mm");

  const amountStyles = isIncoming
    ? { color: "success.tint" }
    : { color: "error.tint" };

  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);
  const menuOpen = Boolean(menuAnchorEl);

  // Return mobile or desktop layout based on screen size
  if (isMobile) {
    return (
      <MobileTransactionLayout
        transaction={transaction}
        onClick={onClick}
        isIncoming={isIncoming}
        displayDate={displayDate}
        amountStyles={amountStyles}
      />
    );
  }

  return (
    <DesktopTransactionLayout
      transaction={transaction}
      isIncoming={isIncoming}
      displayDate={displayDate}
      amountStyles={amountStyles}
      menuAnchorEl={menuAnchorEl}
      setMenuAnchorEl={setMenuAnchorEl}
      menuOpen={menuOpen}
    />
  );
}
