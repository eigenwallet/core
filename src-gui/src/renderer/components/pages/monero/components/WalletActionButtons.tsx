import {
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  ListItemIcon,
  Menu,
  MenuItem,
  TextField,
  Typography,
} from "@mui/material";
import {
  Send as SendIcon,
  SwapHoriz as SwapIcon,
  Restore as RestoreIcon,
  MoreHoriz as MoreHorizIcon,
} from "@mui/icons-material";
import { useState } from "react";
import { setMoneroRestoreHeight } from "renderer/rpc";
import SendTransactionModal from "../SendTransactionModal";
import { useNavigate } from "react-router-dom";
import PromiseInvokeButton from "renderer/components/buttons/PromiseInvokeButton";
import SetRestoreHeightModal from "../SetRestoreHeightModal";
import DfxButton from "./DFXWidget";
import SendButton from "renderer/components/features/wallet/SendButton.mobile";

interface WalletActionButtonsProps {
  balance: {
    unlocked_balance: string;
  };
}

export default function WalletActionButtons({
  balance,
}: WalletActionButtonsProps) {
  const navigate = useNavigate();
  const [restoreHeightDialogOpen, setRestoreHeightDialogOpen] = useState(false);

  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);
  const menuOpen = Boolean(menuAnchorEl);
  const handleMenuClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    setMenuAnchorEl(event.currentTarget);
  };
  const handleMenuClose = () => {
    setMenuAnchorEl(null);
  };

  return (
    <>
      <SetRestoreHeightModal
        open={restoreHeightDialogOpen}
        onClose={() => setRestoreHeightDialogOpen(false)}
      />
      <Box
        sx={{
          display: "flex",
          flexWrap: "wrap",
          gap: 1,
          mb: 2,
          alignItems: "center",
        }}
      >
        <SendButton balance={balance} />
        <Chip
          onClick={() => navigate("/swap")}
          icon={<SwapIcon />}
          label="Swap"
          variant="button"
          clickable
        />
        <DfxButton />

        <IconButton onClick={handleMenuClick}>
          <MoreHorizIcon />
        </IconButton>
        <Menu anchorEl={menuAnchorEl} open={menuOpen} onClose={handleMenuClose}>
          <MenuItem
            onClick={() => {
              setRestoreHeightDialogOpen(true);
              handleMenuClose();
            }}
          >
            <ListItemIcon>
              <RestoreIcon />
            </ListItemIcon>
            <Typography>Restore Height</Typography>
          </MenuItem>
        </Menu>
      </Box>
    </>
  );
}
