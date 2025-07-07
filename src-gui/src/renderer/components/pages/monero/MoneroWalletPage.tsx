import { useEffect, useState } from "react";
import {
  Box,
  Typography,
  CircularProgress,
  Alert,
  TextField,
  Button,
  Card,
  CardContent,
  InputAdornment,
  LinearProgress,
  Stack,
  Divider,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Paper,
  Chip,
  IconButton,
  Tooltip,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
} from "@mui/material";
import {
  Send as SendIcon,
  Refresh as RefreshIcon,
  OpenInNew as OpenInNewIcon,
  AccountBalance as DfxIcon,
} from "@mui/icons-material";
import { open } from "@tauri-apps/plugin-shell";
import ActionableMonospaceTextBox from "../../other/ActionableMonospaceTextBox";
import { PiconeroAmount } from "../../other/Units";
import {
  piconerosToXmr,
  xmrToPiconeros,
  getMoneroTxExplorerUrl,
} from "../../../../utils/conversionUtils";
import { isTestnet } from "store/config";
import PromiseInvokeButton from "../../PromiseInvokeButton";
import { useAppSelector } from "store/hooks";
import {
  updateMoneroSyncProgress,
  initializeMoneroWallet,
  sendMoneroTransaction,
  refreshMoneroWallet,
  dfxAuthenticate,
} from "renderer/rpc";
import DFXSwissLogo from "assets/dfx-logo.svg";

function DFXLogo({ height = 24 }: { height?: number }) {
  return (
    <Box sx={{ backgroundColor: "white", borderRadius: 1, display: "flex", alignItems: "center", padding: 1, height }}>
      <img src={DFXSwissLogo} alt="DFX Swiss" style={{ height: "100%", flex: 1 }} />
    </Box>
  );
}

// Component for DFX button and modal
function DfxButton() {
  const [dfxUrl, setDfxUrl] = useState<string | null>(null);

  const handleOpenDfx = async () => {
    // Get authentication token and URL (this will initialize DFX if needed)
    const response = await dfxAuthenticate();
    setDfxUrl(response.kyc_url);
    return response;
  };

  const handleCloseModal = () => {
    setDfxUrl(null);
  };

  return (
    <>
      <PromiseInvokeButton
        variant="outlined"
        size="small"
        startIcon={<DFXLogo height={12} />}
        onInvoke={handleOpenDfx}
        displayErrorSnackbar={true}
      >
        Fiat Conversion
      </PromiseInvokeButton>

      <Dialog
        open={dfxUrl != null}
        onClose={handleCloseModal}
        maxWidth="lg"
        fullWidth
      >
        <DialogTitle>
          <Box
            sx={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <DFXLogo />
            <Button onClick={handleCloseModal} variant="outlined">
              Close
            </Button>
          </Box>
        </DialogTitle>
        <DialogContent sx={{ p: 0, height: "min(40rem, 80vh)" }}>
          {dfxUrl && (
            <iframe
              src={dfxUrl}
              style={{
                width: "100%",
                height: "100%",
                border: "none",
              }}
              title="DFX Swiss"
            />
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}

// Component for displaying wallet address and balance
function WalletOverview({ mainAddress, balance, isRefreshing, onRefresh }) {
  return (
    <>
      {/* Balance */}
      {balance != null && (
        <Card>
          <CardContent
            sx={{ display: "flex", flexDirection: "column", gap: 2 }}
          >
            <Box
              sx={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <Typography variant="h6">Balance</Typography>
              <Stack direction="row" spacing={1}>
                <DfxButton />
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={
                    isRefreshing ? (
                      <CircularProgress size={16} />
                    ) : (
                      <RefreshIcon />
                    )
                  }
                  onClick={onRefresh}
                  disabled={isRefreshing}
                >
                  {isRefreshing ? "Refreshing..." : "Refresh"}
                </Button>
              </Stack>
            </Box>
            <Divider />
            <Box
              sx={{
                display: "flex",
                justifyContent: "space-around",
                alignItems: "center",
              }}
            >
              <Box sx={{ textAlign: "center" }}>
                <Typography variant="body2" color="text.secondary">
                  Unlocked
                </Typography>
                <Typography variant="h5" color="success.main">
                  <PiconeroAmount
                    amount={parseFloat(balance.unlocked_balance)}
                  />
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  Available to spend
                </Typography>
              </Box>
              <Divider orientation="vertical" flexItem />
              <Box sx={{ textAlign: "center" }}>
                <Typography variant="body2" color="text.secondary">
                  Pending
                </Typography>
                <Typography variant="h5" color="warning.main">
                  <PiconeroAmount
                    amount={
                      parseFloat(balance.total_balance) -
                      parseFloat(balance.unlocked_balance)
                    }
                  />
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  Locked ({"<"}10 confirmations)
                </Typography>
              </Box>
            </Box>
          </CardContent>
        </Card>
      )}
      {/* Primary Address */}
      {mainAddress && <ActionableMonospaceTextBox content={mainAddress} />}
    </>
  );
}

// Component for displaying sync progress
function SyncProgress({ syncProgress }) {
  if (!syncProgress) return null;

  return (
    <Card>
      <CardContent>
        <Stack spacing={1}>
          <Box
            sx={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <Typography variant="body2" color="text.secondary">
              Block {syncProgress.current_block.toLocaleString()} of{" "}
              {syncProgress.target_block.toLocaleString()}
            </Typography>
            <Typography variant="body2" color="text.secondary">
              {syncProgress.progress_percentage.toFixed(2)}%
            </Typography>
          </Box>
          <LinearProgress
            variant="determinate"
            value={syncProgress.progress_percentage}
            sx={{ height: 8, borderRadius: 4 }}
          />
          {syncProgress.progress_percentage < 100 && (
            <Typography variant="body2" color="text.secondary">
              Wallet is synchronizing with the Monero network...
            </Typography>
          )}
          {syncProgress.progress_percentage >= 100 && (
            <Typography variant="body2" color="success.main">
              Wallet is fully synchronized
            </Typography>
          )}
        </Stack>
      </CardContent>
    </Card>
  );
}

// Component for sending transactions
function SendTransaction({ balance, onSend }) {
  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");

  const handleSend = async () => {
    if (!sendAddress || !sendAmount) {
      throw new Error("Address and amount are required");
    }

    return onSend({
      address: sendAddress,
      amount: xmrToPiconeros(parseFloat(sendAmount)),
    });
  };

  const handleSendSuccess = () => {
    // Clear form after successful send
    setSendAddress("");
    setSendAmount("");
  };

  const handleMaxAmount = () => {
    if (balance?.unlocked_balance) {
      // TODO: We need to use a real fee here and call sweep(...) instead of just subtracting a fixed amount
      const unlocked = parseFloat(balance.unlocked_balance);
      const maxAmount = piconerosToXmr(unlocked - 10000000000); // Subtract ~0.01 XMR for fees
      setSendAmount(Math.max(0, maxAmount).toString());
    }
  };

  const handleClear = () => {
    setSendAddress("");
    setSendAmount("");
  };

  return (
    <Card>
      <CardContent sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Typography variant="h6">Transfer</Typography>
        <Divider />
        <Stack spacing={2}>
          <TextField
            fullWidth
            label="Pay to"
            placeholder="Monero address"
            value={sendAddress}
            onChange={(e) => setSendAddress(e.target.value)}
          />

          <Stack direction="row" spacing={1}>
            <TextField
              fullWidth
              label="Amount"
              placeholder="0.0"
              value={sendAmount}
              onChange={(e) => setSendAmount(e.target.value)}
              type="number"
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">XMR</InputAdornment>
                ),
              }}
            />
            <Button
              variant="outlined"
              onClick={handleMaxAmount}
              disabled={!balance?.unlocked_balance}
            >
              Max
            </Button>
          </Stack>

          <Stack direction="row" spacing={1} justifyContent="flex-end">
            <Button variant="outlined" onClick={handleClear}>
              Clear
            </Button>
            <PromiseInvokeButton
              variant="contained"
              color="primary"
              endIcon={<SendIcon />}
              onInvoke={handleSend}
              onSuccess={handleSendSuccess}
              disabled={!sendAddress || !sendAmount}
              displayErrorSnackbar={true}
              sx={{ minWidth: 100 }}
            >
              Send
            </PromiseInvokeButton>
          </Stack>
        </Stack>
      </CardContent>
    </Card>
  );
}

// Component for displaying transaction history
function TransactionHistory({ history }) {
  if (!history || !history.transactions || history.transactions.length === 0) {
    return (
      <Card>
        <CardContent>
          <Typography variant="h6" gutterBottom>
            Transaction History
          </Typography>
          <Typography variant="body2" color="text.secondary">
            No transactions found
          </Typography>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardContent>
        <Typography variant="h6" gutterBottom>
          Transaction History
        </Typography>
        <TableContainer component={Paper} variant="outlined">
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>Amount</TableCell>
                <TableCell>Fee</TableCell>
                <TableCell align="right">Confirmations</TableCell>
                <TableCell align="center">Explorer</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {history.transactions.map((tx, index) => (
                <TableRow key={index}>
                  <TableCell>
                    <Stack direction="row" spacing={1} alignItems="center">
                      <PiconeroAmount amount={tx.amount} />
                      <Chip
                        label={tx.amount >= 0 ? "Received" : "Sent"}
                        color={tx.amount >= 0 ? "success" : "default"}
                        size="small"
                      />
                    </Stack>
                  </TableCell>
                  <TableCell>
                    <PiconeroAmount amount={tx.fee} />
                  </TableCell>
                  <TableCell align="right">
                    <Chip
                      label={tx.confirmations}
                      color={tx.confirmations >= 10 ? "success" : "warning"}
                      size="small"
                    />
                  </TableCell>
                  <TableCell align="center">
                    {tx.tx_hash && (
                      <Tooltip title="View on block explorer">
                        <IconButton
                          size="small"
                          onClick={() => {
                            const url = getMoneroTxExplorerUrl(
                              tx.tx_hash,
                              isTestnet(),
                            );
                            open(url);
                          }}
                        >
                          <OpenInNewIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      </CardContent>
    </Card>
  );
}

// Main MoneroWalletPage component
export default function MoneroWalletPage() {
  const { mainAddress, balance, syncProgress, history, isRefreshing } =
    useAppSelector((state) => state.wallet.state);

  // Auto-refresh sync progress every 5 seconds if not fully synced
  useEffect(() => {
    if (!syncProgress || syncProgress.progress_percentage >= 100) {
      return;
    }

    const interval = setInterval(() => {
      updateMoneroSyncProgress();
    }, 5000);

    return () => clearInterval(interval);
  }, [syncProgress]);

  useEffect(() => {
    initializeMoneroWallet();
  }, []);

  const handleSendTransaction = async (transactionData) => {
    await sendMoneroTransaction(transactionData);
  };

  return (
    <Box
      sx={{
        maxWidth: 800,
        mx: "auto",
        display: "flex",
        flexDirection: "column",
        gap: 2,
        pb: 2,
      }}
    >
      <WalletOverview
        mainAddress={mainAddress}
        balance={balance}
        isRefreshing={isRefreshing}
        onRefresh={refreshMoneroWallet}
      />

      <SyncProgress syncProgress={syncProgress} />

      <SendTransaction balance={balance} onSend={handleSendTransaction} />

      <TransactionHistory history={history} />
    </Box>
  );
}
