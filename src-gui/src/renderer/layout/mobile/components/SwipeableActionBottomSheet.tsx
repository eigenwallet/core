import { useState } from "react";
import {
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Box,
  Typography,
  useTheme,
  Drawer,
} from "@mui/material";
import AccountBalanceWalletIcon from "@mui/icons-material/AccountBalanceWallet";
import RestoreIcon from "@mui/icons-material/Restore";
import WithdrawDialog from "renderer/components/modal/wallet/WithdrawDialog";
import SetRestoreHeightModal from "renderer/components/pages/monero/SetRestoreHeightModal";
import SeedPhraseButton from "renderer/components/pages/monero/SeedPhraseButton";
import { GetMoneroSeedResponse, GetRestoreHeightResponse } from "models/tauriModel";
import SeedPhraseModal from "renderer/components/pages/monero/SeedPhraseModal";
import { Key as KeyIcon } from "@mui/icons-material";
import { getMoneroSeedAndRestoreHeight } from "renderer/rpc";

interface SwipeableActionBottomSheetProps {
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
}

export default function SwipeableActionBottomSheet({
  open,
  onOpen,
  onClose,
}: SwipeableActionBottomSheetProps) {
  const theme = useTheme();
  const [withdrawDialogOpen, setWithdrawDialogOpen] = useState(false);
  const [restoreHeightDialogOpen, setRestoreHeightDialogOpen] = useState(false);
  const [seedPhrase, setSeedPhrase] = useState<
    [GetMoneroSeedResponse, GetRestoreHeightResponse] | null
  >(null);

  const handleWithdrawClick = () => {
    onClose();
    setWithdrawDialogOpen(true);
  };

  const handleRestoreHeightClick = () => {
    onClose();
    setRestoreHeightDialogOpen(true);
  };

  const handleSeedPhraseClick = async () => {
    onClose();
    const seedPhrase = await getMoneroSeedAndRestoreHeight();
    setSeedPhrase(seedPhrase);
  };

  return (
    <>
      < Drawer
        anchor="bottom"
        open={open}
        onClose={onClose}
        sx={{
          "& .MuiDrawer-paper": {
            borderTopLeftRadius: 16,
            borderTopRightRadius: 16,
            maxHeight: "50vh",
          },
        }}
      >
        <Box
          sx={{
            px: 2,
            py: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            borderBottom: `1px solid ${theme.palette.divider}`,
          }}
        >
          <Typography variant="h6" component="div">
            More Actions
          </Typography>
        </Box>
        
        <List sx={{ py: 0 }}>
          <ListItem
            onClick={handleWithdrawClick}
            sx={{
              cursor: "pointer",
              "&:hover": {
                backgroundColor: theme.palette.action.hover,
              },
              py: 2,
            }}
          >
            <ListItemIcon>
              <AccountBalanceWalletIcon />
            </ListItemIcon>
            <ListItemText primary="Withdraw Bitcoin" />
          </ListItem>
          
          <ListItem
            onClick={handleRestoreHeightClick}
            sx={{
              cursor: "pointer",
              "&:hover": {
                backgroundColor: theme.palette.action.hover,
              },
              py: 2,
            }}
          >
            <ListItemIcon>
              <RestoreIcon />
            </ListItemIcon>
            <ListItemText primary="Restore Height" />
          </ListItem>
          <ListItem
            onClick={handleSeedPhraseClick}
            sx={{
              cursor: "pointer",
              "&:hover": {
                backgroundColor: theme.palette.action.hover,
              },
              py: 2,
            }}
          >
            <ListItemIcon>
              <KeyIcon />
            </ListItemIcon>
            <ListItemText primary="Seed Phrase" />
          </ListItem>
        </List>
      </Drawer>

      {/* Dialogs */}
      <WithdrawDialog
        open={withdrawDialogOpen}
        onClose={() => setWithdrawDialogOpen(false)}
      />
      <SetRestoreHeightModal
        open={restoreHeightDialogOpen}
        onClose={() => setRestoreHeightDialogOpen(false)}
      />
      <SeedPhraseModal
        onClose={() => setSeedPhrase(null)}
        seed={seedPhrase}
      />
    </>
  );
}
