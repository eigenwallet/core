import {
  Box,
  Typography,
} from "@mui/material";
import { TransactionDirection, TransactionInfo } from "models/tauriModel";
import {
  CallReceived as IncomingIcon,
} from "@mui/icons-material";
import { CallMade as OutgoingIcon } from "@mui/icons-material";
import {
  FiatPiconeroAmount,
  PiconeroAmount,
} from "renderer/components/other/Units";
import ConfirmationsBadge from "./ConfirmationsBadge";
import MoneroIcon from "renderer/components/icons/MoneroIcon";
import dayjs from "dayjs";

interface TransactionItemMobileProps {
  transaction: TransactionInfo;
  onClick?: () => void;
}

export default function TransactionItemMobile({ transaction, onClick }: TransactionItemMobileProps) {
  const isIncoming = transaction.direction === TransactionDirection.In;
  const displayDate = dayjs(transaction.timestamp * 1000).format("MMM DD, HH:mm");
  const amountStyles = isIncoming
    ? { color: "success.tint" }
    : { color: "error.tint" };

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
        
        {/* Date or Confirmations Badge */}
        {transaction.confirmations >= 10 ? (
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
        ) : (
          <Box sx={{position: "relative", top: 3}}>
          <ConfirmationsBadge confirmations={transaction.confirmations} />
          </Box>
        )}
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
