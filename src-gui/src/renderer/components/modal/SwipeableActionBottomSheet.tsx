import React, { useState } from "react";
import {
  SwipeableDrawer,
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Box,
  Typography,
  IconButton,
  useTheme,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import AccountBalanceWalletIcon from "@mui/icons-material/AccountBalanceWallet";
import RestoreIcon from "@mui/icons-material/Restore";
import WithdrawDialog from "./wallet/WithdrawDialog";
import SetRestoreHeightModal from "../pages/monero/SetRestoreHeightModal";
import { useIsMobile } from "../../../utils/useIsMobile";

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
  const isMobile = useIsMobile();
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

  // Only show on mobile
  if (!isMobile) {
    return null;
  }

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
      <SwipeableDrawer
        anchor="bottom"
        open={open}
        onClose={onClose}
        onOpen={onOpen}
        swipeAreaWidth={56}
        disableSwipeToOpen={false}
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
          <IconButton
            edge="end"
            onClick={onClose}
            aria-label="close"
            size="small"
          >
            <CloseIcon />
          </IconButton>
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
      </SwipeableDrawer>

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