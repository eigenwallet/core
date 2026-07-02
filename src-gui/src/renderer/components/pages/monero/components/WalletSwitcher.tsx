import {
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  ListItemIcon,
  Menu,
  MenuItem,
  Typography,
} from "@mui/material";
import {
  AccountBalanceWallet as WalletIcon,
  Add as AddIcon,
  ExpandMore as ExpandMoreIcon,
  FolderOpen as FolderOpenIcon,
} from "@mui/icons-material";
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { getRecentWallets, setPendingWallet } from "renderer/rpc";
import { useIsSwapRunningAndHasFundsLocked } from "store/hooks";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";

function walletFileName(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}

// Switching opens a specific wallet; creating restarts into the wallet setup
// chooser (a ShowChooser marker also neutralizes any stale Open marker left by
// a previously failed switch).
type PendingAction = { kind: "switch"; path: string } | { kind: "create" };

// Shows the current wallet and lets the user open a different one. The most
// recently accessed wallet is the one currently open, so the rest of the
// recent list are the other wallets to switch to. The chosen wallet is recorded
// and the app relaunched, which opens it from a clean state on startup.
export default function WalletSwitcher() {
  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);
  const [recentWallets, setRecentWallets] = useState<string[]>([]);
  // Action the user picked, awaiting confirmation before the app relaunches.
  // Kept while the dialog fades out so the copy doesn't flicker; `confirmOpen`
  // alone controls visibility.
  const [pending, setPending] = useState<PendingAction | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const swapRunning = useIsSwapRunningAndHasFundsLocked();

  const refreshRecentWallets = () => {
    getRecentWallets()
      .then(setRecentWallets)
      .catch((e) => console.error("Failed to load recent wallets", e));
  };

  useEffect(refreshRecentWallets, []);

  const currentWallet = recentWallets[0];
  const otherWallets = recentWallets.slice(1);

  const openMenu = (event: React.MouseEvent<HTMLElement>) => {
    refreshRecentWallets();
    setAnchorEl(event.currentTarget);
  };

  const requestConfirmation = (action: PendingAction) => {
    setPending(action);
    setConfirmOpen(true);
  };

  const confirmPending = async () => {
    if (pending === null) return;
    await setPendingWallet(
      pending.kind === "switch"
        ? { type: "Open", content: { wallet_path: pending.path } }
        : { type: "ShowChooser" },
    );
    try {
      await relaunch();
    } catch (e) {
      // Never leave an Open marker behind when the relaunch failed: a later
      // manual launch would silently open a wallet the user no longer expects.
      await setPendingWallet({ type: "ShowChooser" }).catch(() => {});
      throw e;
    }
  };

  const chooseOther = async () => {
    const selected = await open({ multiple: false, directory: false });
    if (!selected) return;
    // Users commonly pick the `<name>.keys` file; the wallet is the file
    // without that extension.
    requestConfirmation({
      kind: "switch",
      path: selected.replace(/\.keys$/, ""),
    });
  };

  return (
    <>
      <Chip
        icon={<WalletIcon />}
        deleteIcon={<ExpandMoreIcon />}
        onDelete={openMenu}
        onClick={openMenu}
        label={currentWallet ? walletFileName(currentWallet) : "Wallet"}
        variant="button"
        clickable
        sx={{
          // Bookmark tab: flush to the top edge, square on top, rounded
          // below, with a small downward notch tail.
          position: "relative",
          height: 36,
          maxWidth: 220,
          bgcolor: "background.paper",
          boxShadow: 3,
          borderTopLeftRadius: 0,
          borderTopRightRadius: 0,
          borderBottomLeftRadius: 12,
          borderBottomRightRadius: 12,
          "&:hover": {
            bgcolor: "background.paper",
            boxShadow: 4,
          },
          "&::after": {
            content: '""',
            position: "absolute",
            bottom: 0,
            left: "50%",
            transform: "translate(-50%, 60%)",
            borderLeft: "6px solid transparent",
            borderRight: "6px solid transparent",
            borderTop: "6px solid",
            borderTopColor: "background.paper",
          },
        }}
      />
      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {otherWallets.map((path) => (
          <MenuItem
            key={path}
            onClick={() => {
              setAnchorEl(null);
              requestConfirmation({ kind: "switch", path });
            }}
          >
            <ListItemIcon>
              <WalletIcon />
            </ListItemIcon>
            <Typography>{walletFileName(path)}</Typography>
          </MenuItem>
        ))}
        {otherWallets.length > 0 && <Divider />}
        <MenuItem
          onClick={() => {
            setAnchorEl(null);
            chooseOther();
          }}
        >
          <ListItemIcon>
            <FolderOpenIcon />
          </ListItemIcon>
          <Typography>Other wallet…</Typography>
        </MenuItem>
        <MenuItem
          onClick={() => {
            setAnchorEl(null);
            requestConfirmation({ kind: "create" });
          }}
        >
          <ListItemIcon>
            <AddIcon />
          </ListItemIcon>
          <Typography>Create new wallet…</Typography>
        </MenuItem>
      </Menu>
      <Dialog
        open={confirmOpen}
        onClose={() => setConfirmOpen(false)}
        TransitionProps={{ onExited: () => setPending(null) }}
        maxWidth="xs"
        fullWidth
      >
        <DialogTitle>
          {pending?.kind === "create" ? "Create new wallet" : "Switch wallet"}
        </DialogTitle>
        <DialogContent>
          <DialogContentText>
            {pending?.kind === "create"
              ? "The app will restart so you can set up a new wallet."
              : `The app will restart to open "${
                  pending ? walletFileName(pending.path) : ""
                }".`}{" "}
            {swapRunning
              ? "A swap with locked funds is currently running. It will be interrupted, and its Monero will arrive in the wallet it was started with — not the one you are switching to."
              : "Any running operations will be interrupted."}
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmOpen(false)} color="inherit">
            Cancel
          </Button>
          <PromiseInvokeButton
            variant="contained"
            onInvoke={confirmPending}
            displayErrorSnackbar
          >
            {pending?.kind === "create" ? "Restart" : "Restart & switch"}
          </PromiseInvokeButton>
        </DialogActions>
      </Dialog>
    </>
  );
}
