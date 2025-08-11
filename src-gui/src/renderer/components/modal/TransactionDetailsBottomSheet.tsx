import React from "react";
import {
  Box,
  Drawer,
  IconButton,
  Typography,
  Stack,
  Chip,
  useTheme,
} from "@mui/material";
import {
  ArrowBack as ArrowBackIcon,
  ContentCopy as CopyIcon,
} from "@mui/icons-material";
import { TransactionDirection, TransactionInfo } from "models/tauriModel";
import {
  FiatPiconeroAmount,
  PiconeroAmount,
} from "renderer/components/other/Units";
import dayjs from "dayjs";

interface TransactionDetailsBottomSheetProps {
  open: boolean;
  onClose: () => void;
  transaction: TransactionInfo | null;
}

export default function TransactionDetailsBottomSheet({
  open,
  onClose,
  transaction,
}: TransactionDetailsBottomSheetProps) {
  const theme = useTheme();

  if (!transaction) return null;

  const isIncoming = transaction.direction === TransactionDirection.In;
  const displayDate = dayjs(transaction.timestamp * 1000).format("MMM Do YYYY, HH:mm");
  const transactionType = isIncoming ? "Received Monero" : "Sent Monero";

  const handleCopyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    // Could add a toast notification here
  };

  const truncateAddress = (address: string, length: number = 12) => {
    if (address.length <= length * 2) return address;
    return `${address.slice(0, length)}...${address.slice(-length)}`;
  };

  // Placeholder address - in real implementation this would come from transaction data
  const fromAddress = "4A1...Bz9"; // Placeholder

  return (
    <Drawer
      anchor="bottom"
      open={open}
      onClose={onClose}
      PaperProps={{
        sx: {
          borderTopLeftRadius: 16,
          borderTopRightRadius: 16,
          maxHeight: "80vh",
          backgroundColor: "background.paper",
        },
      }}
    >
      <Box sx={{ p: 3, pb: 4 }}>
        {/* Header */}
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            gap: 2,
            mb: 3,
          }}
        >
          <IconButton onClick={onClose} size="small">
            <ArrowBackIcon />
          </IconButton>
          <Typography variant="h6" sx={{ fontWeight: 600 }}>
            Transaction Details
          </Typography>
        </Box>

        {/* Transaction Summary Section */}
        <Box
          sx={{
            textAlign: "center",
            mb: 4,
            py: 3,
          }}
        >
          {/* Transaction type */}
          <Typography
            variant="h5"
            sx={{
              fontWeight: 600,
              mb: 1,
              color: "text.primary",
            }}
          >
            {transactionType}
          </Typography>

          {/* Date */}
          <Typography
            variant="body2"
            sx={{
              color: "text.secondary",
              mb: 3,
            }}
          >
            {displayDate}
          </Typography>

          {/* Amount in XMR */}
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 0.5,
              mb: 1,
            }}
          >
            <Typography
              variant="h4"
              sx={{
                fontWeight: "bold",
                color: isIncoming ? "success.main" : "error.main",
                opacity: !isIncoming ? 1 : 0,
              }}
            >
              {!isIncoming ? "−" : ""}
            </Typography>
            <Typography
              variant="h4"
              sx={{
                fontWeight: "bold",
                color: isIncoming ? "success.main" : "error.main",
              }}
            >
              <PiconeroAmount
                amount={transaction.amount}
                labelStyles={{ fontSize: 24, ml: -0.5 }}
                disableTooltip
              />
            </Typography>
          </Box>

          {/* EUR equivalent */}
          <Typography
            variant="body1"
            sx={{
              color: "text.secondary",
            }}
          >
            <FiatPiconeroAmount amount={transaction.amount} />
          </Typography>
        </Box>

        {/* Transaction Details Section */}
        <Stack spacing={3}>
          {/* From field */}
          <Box>
            <Typography
              variant="caption"
              sx={{
                color: "text.secondary",
                textTransform: "uppercase",
                fontWeight: 600,
                letterSpacing: 1,
                mb: 1,
                display: "block",
              }}
            >
              From
            </Typography>
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                py: 1.5,
                px: 2,
                backgroundColor: "action.hover",
                borderRadius: 1,
              }}
            >
              <Typography
                variant="body2"
                sx={{
                  fontFamily: "monospace",
                  color: "text.primary",
                }}
              >
                {truncateAddress(fromAddress)}
              </Typography>
              <IconButton
                size="small"
                onClick={() => handleCopyToClipboard(fromAddress)}
              >
                <CopyIcon fontSize="small" />
              </IconButton>
            </Box>
          </Box>

          {/* Transaction ID field */}
          <Box>
            <Typography
              variant="caption"
              sx={{
                color: "text.secondary",
                textTransform: "uppercase",
                fontWeight: 600,
                letterSpacing: 1,
                mb: 1,
                display: "block",
              }}
            >
              Transaction ID
            </Typography>
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                py: 1.5,
                px: 2,
                backgroundColor: "action.hover",
                borderRadius: 1,
              }}
            >
              <Typography
                variant="body2"
                sx={{
                  fontFamily: "monospace",
                  color: "text.primary",
                }}
              >
                {truncateAddress(transaction.tx_hash, 16)}
              </Typography>
              <IconButton
                size="small"
                onClick={() => handleCopyToClipboard(transaction.tx_hash)}
              >
                <CopyIcon fontSize="small" />
              </IconButton>
            </Box>
          </Box>

          {/* Fees field */}
          <Box>
            <Typography
              variant="caption"
              sx={{
                color: "text.secondary",
                textTransform: "uppercase",
                fontWeight: 600,
                letterSpacing: 1,
                mb: 1,
                display: "block",
              }}
            >
              Fees
            </Typography>
            <Box>
              <Chip
                label={
                  <PiconeroAmount
                    amount={transaction.fee}
                    labelStyles={{ fontSize: 12 }}
                    disableTooltip
                  />
                }
                sx={{
                  borderRadius: 20,
                  backgroundColor: "action.hover",
                  color: "text.primary",
                  height: 32,
                  "& .MuiChip-label": {
                    px: 2,
                  },
                }}
              />
            </Box>
          </Box>
        </Stack>
      </Box>
    </Drawer>
  );
}