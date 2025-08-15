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

  const handleWithdrawClick = () => {
    onClose();
    setWithdrawDialogOpen(true);
  };

  const handleRestoreHeightClick = () => {
    onClose();
    setRestoreHeightDialogOpen(true);
  };

  const actions = [
    {
      id: "withdraw-bitcoin",
      label: "Withdraw Bitcoin",
      icon: <AccountBalanceWalletIcon />,
      onClick: handleWithdrawClick,
    },
    {
      id: "restore-height",
      label: "Restore Height",
      icon: <RestoreIcon />,
      onClick: handleRestoreHeightClick,
    },
  ];

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
          {actions.map((action) => (
            <ListItem
              key={action.id}
              onClick={action.onClick}
              sx={{
                cursor: "pointer",
                "&:hover": {
                  backgroundColor: theme.palette.action.hover,
                },
                py: 2,
              }}
            >
              <ListItemIcon>{action.icon}</ListItemIcon>
              <ListItemText primary={action.label} />
            </ListItem>
          ))}
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
    </>
  );
}
