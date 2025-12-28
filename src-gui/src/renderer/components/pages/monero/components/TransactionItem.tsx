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
import { openUrl } from "@tauri-apps/plugin-opener";
import dayjs from "dayjs";
import { useState } from "react";
import { useMoneroMainAddress, useMoneroSubaddresses } from "store/hooks";
import _ from "lodash";

interface TransactionItemProps {
  transaction: TransactionInfo;
}

export default function TransactionItem({ transaction }: TransactionItemProps) {
  const isIncoming = transaction.direction === TransactionDirection.In;
  const moneroMainAddress = useMoneroMainAddress();
  const subaddresses = useMoneroSubaddresses();
  const subaddress = subaddresses.find(
    (s) => s.address === transaction.received_address,
  );

  let addressLabel: string | null = null;
  if (isIncoming && transaction.received_address) {
    if (subaddress && subaddress.label.length > 0) {
      addressLabel = subaddress.label;
    } else {
      addressLabel = _.truncate(transaction.received_address, { length: 8 });
    }
  }

  const shouldShowSubaddressChip = Boolean(
    isIncoming &&
    transaction.received_address &&
    (transaction.received_address.trim() !== moneroMainAddress?.trim() ||
      (subaddress?.account_index === 0 && subaddress?.address_index !== 0)) &&
    subaddress &&
    addressLabel,
  );

  const displayDate = dayjs(transaction.timestamp * 1000).format(
    "MMM DD YYYY, HH:mm",
  );

  const amountStyles = isIncoming
    ? { color: "success.tint" }
    : { color: "error.tint" };

  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);
  const menuOpen = Boolean(menuAnchorEl);

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
            rowGap: 0.25,
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

          <Typography variant="caption" sx={{ gridColumn: "2 / 3" }}>
            <FiatPiconeroAmount amount={transaction.amount} />
          </Typography>

          {shouldShowSubaddressChip && subaddress && addressLabel && (
            <Chip
              size="small"
              variant="outlined"
              label={
                <Typography noWrap>
                  {`Address #${subaddress.address_index}`}
                  <i>{`"${addressLabel}"`}</i>
                </Typography>
              }
              sx={{
                gridColumn: "2 / 3",
                maxWidth: 220,
                "& .MuiChip-label": {
                  width: "100%",
                  display: "block",
                  textOverflow: "ellipsis",
                  overflow: "hidden",
                },
              }}
              title={transaction.received_address}
            />
          )}
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
              openUrl(getMoneroTxExplorerUrl(transaction.tx_hash, isTestnet()));
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
