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
import { getMoneroTxExplorerUrl } from "utils/conversionUtils";
import { isTestnet } from "store/config";
import { open } from "@tauri-apps/plugin-shell";
import dayjs from "dayjs";
import { useState, useMemo, useCallback } from "react";

interface TransactionItemProps {
  transaction: TransactionInfo;
}

export default function TransactionItem({ transaction }: TransactionItemProps) {
  const isIncoming = transaction.direction === TransactionDirection.In;
  const displayDate = useMemo(
    () => dayjs(transaction.timestamp * 1000).format("MMM DD YYYY, HH:mm"),
    [transaction.timestamp],
  );

  // Memoize amountStyles to avoid creating new object on every render
  const amountStyles = useMemo(
    () => (isIncoming ? { color: "success.tint" } : { color: "error.tint" }),
    [isIncoming],
  );

  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);
  const menuOpen = Boolean(menuAnchorEl);

  const handleMenuOpen = useCallback(
    (event: React.MouseEvent<HTMLElement>) => {
      setMenuAnchorEl(event.currentTarget);
    },
    [],
  );

  const handleMenuClose = useCallback(() => {
    setMenuAnchorEl(null);
  }, []);

  const handleCopyTxId = useCallback(() => {
    navigator.clipboard.writeText(transaction.tx_hash);
    setMenuAnchorEl(null);
  }, [transaction.tx_hash]);

  const handleViewExplorer = useCallback(() => {
    open(getMoneroTxExplorerUrl(transaction.tx_hash, isTestnet()));
    setMenuAnchorEl(null);
  }, [transaction.tx_hash]);

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
          <Typography variant="caption" sx={{ gridArea: "2 / 2" }}>
            <FiatPiconeroAmount amount={transaction.amount} />
          </Typography>
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
        <IconButton onClick={handleMenuOpen}>
          <MoreVertIcon />
        </IconButton>
        <Menu
          anchorEl={menuAnchorEl}
          open={menuOpen}
          onClose={handleMenuClose}
        >
          <MenuItem onClick={handleCopyTxId}>
            <Typography>Copy Transaction ID</Typography>
          </MenuItem>
          <MenuItem onClick={handleViewExplorer}>
            <Typography>View on Explorer</Typography>
          </MenuItem>
        </Menu>
      </Box>
    </Box>
  );
}
