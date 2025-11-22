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
  LockOutline as LockOutlineIcon,
} from "@mui/icons-material";
import { useState } from "react";
import { setMoneroRestoreHeight } from "renderer/rpc";
import SendTransactionModal from "../SendTransactionModal";
import { useNavigate } from "react-router-dom";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import SetRestoreHeightModal from "../SetRestoreHeightModal";
import SetPasswordModal from "../SetPasswordModal";
import SeedPhraseButton from "../SeedPhraseButton";
import SeedPhraseModal from "../SeedPhraseModal";
import DfxButton from "./DFXWidget";
import {
  GetMoneroSeedResponse,
  GetRestoreHeightResponse,
  GetMoneroBalanceResponse,
} from "models/tauriModel";

interface WalletActionButtonsProps {
  balance: GetMoneroBalanceResponse;
}

export default function WalletActionButtons({
  balance,
}: WalletActionButtonsProps) {
  const navigate = useNavigate();

  const [sendDialogOpen, setSendDialogOpen] = useState(false);
  const [restoreHeightDialogOpen, setRestoreHeightDialogOpen] = useState(false);
  const [setPasswordDialogOpen, setSetPasswordDialogOpen] = useState(false);
  const [seedPhrase, setSeedPhrase] = useState<
    [GetMoneroSeedResponse, GetRestoreHeightResponse] | null
  >(null);

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
      <SetPasswordModal
        open={setPasswordDialogOpen}
        onClose={() => setSetPasswordDialogOpen(false)}
      />
      <SeedPhraseModal onClose={() => setSeedPhrase(null)} seed={seedPhrase} />
      <SendTransactionModal
        wallet="monero"
        unlocked_balance={balance.unlocked_balance}
        open={sendDialogOpen}
        onClose={() => setSendDialogOpen(false)}
      />
      <Box sx={{ display: "flex", justifyContent: "space-between" }}>
        <Box
          sx={{
            display: "flex",
            flexWrap: "wrap",
            gap: 1,
            alignItems: "center",
          }}
        >
          <Chip
            icon={<SendIcon />}
            label="Send"
            variant="button"
            clickable
            onClick={() => setSendDialogOpen(true)}
            disabled={!balance || balance.unlocked_balance <= 0}
          />
          <Chip
            onClick={() => navigate("/swap")}
            icon={<SwapIcon />}
            label="Swap"
            variant="button"
            clickable
          />
          <DfxButton />
        </Box>
        <Box>
          <IconButton onClick={handleMenuClick}>
            <MoreHorizIcon />
          </IconButton>
          <Menu
            anchorEl={menuAnchorEl}
            open={menuOpen}
            onClose={handleMenuClose}
          >
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
            <SeedPhraseButton
              onMenuClose={handleMenuClose}
              onSeedPhraseSuccess={setSeedPhrase}
            />
            <MenuItem
              onClick={() => {
                setSetPasswordDialogOpen(true);
                handleMenuClose();
              }}
            >
              <ListItemIcon>
                <LockOutlineIcon />
              </ListItemIcon>
              <Typography>Change Password</Typography>
            </MenuItem>
          </Menu>
        </Box>
      </Box>
    </>
  );
}
