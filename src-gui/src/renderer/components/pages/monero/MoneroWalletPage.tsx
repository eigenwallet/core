import { useEffect, useState } from "react";
import { useSelector } from "react-redux";
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
} from "@mui/material";
import { Send as SendIcon, Refresh as RefreshIcon } from "@mui/icons-material";
import {
  initializeMoneroWallet,
  refreshMoneroWallet,
  sendMoneroTransaction,
  updateMoneroSyncProgress,
} from "../../../rpc";
import ActionableMonospaceTextBox from "../../other/ActionableMonospaceTextBox";
import { PiconeroAmount } from "../../other/Units";
import { RootState } from "../../../store/storeRenderer";
import {
  piconerosToXmr,
  xmrToPiconeros,
} from "../../../../utils/conversionUtils";

// Component for displaying wallet address and balance
function WalletOverview({ mainAddress, balance, isRefreshing, onRefresh }) {
  return (
    <>
      {/* Balance */}
      {balance && (
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
            </Box>
            <Divider />
            <Box
              sx={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
              }}
            >
              <Box>
                <Typography variant="body2" color="text.secondary">
                  Confirmed
                </Typography>
                <Typography variant="h5">
                  <PiconeroAmount
                    amount={
                      parseFloat(balance.total_balance) -
                      parseFloat(balance.unlocked_balance)
                    }
                  />
                </Typography>
              </Box>
              <Divider orientation="vertical" flexItem />
              <Box>
                <Typography variant="body2" color="text.secondary">
                  Unconfirmed
                </Typography>
                <Typography variant="h5" color="primary">
                  <PiconeroAmount
                    amount={parseFloat(balance.unlocked_balance)}
                  />
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
function SendTransaction({ balance, isSending, onSend }) {
  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");

  const handleSend = async () => {
    if (!sendAddress || !sendAmount) return;

    await onSend({
      address: sendAddress,
      amount: xmrToPiconeros(parseFloat(sendAmount)),
    });

    // Clear form after successful send
    setSendAddress("");
    setSendAmount("");
  };

  const handleMaxAmount = () => {
    if (balance?.unlocked_balance) {
      // Convert piconero to XMR and leave some for fees
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
            <Button
              variant="outlined"
              onClick={handleClear}
              disabled={isSending}
            >
              Clear
            </Button>
            <Button
              variant="contained"
              color="primary"
              endIcon={<SendIcon />}
              onClick={handleSend}
              disabled={!sendAddress || !sendAmount || isSending}
              sx={{ minWidth: 100 }}
            >
              {isSending ? <CircularProgress size={20} /> : "Send"}
            </Button>
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
  const {
    mainAddress,
    balance,
    syncProgress,
    history,
    isLoading,
    isRefreshing,
    isSending,
    error,
    sendResult,
  } = useSelector((state: RootState) => state.wallet.state);

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

  if (isLoading) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", mt: 4 }}>
        <CircularProgress />
      </Box>
    );
  }

  return (
    <Box
      sx={{
        maxWidth: 800,
        mx: "auto",
        display: "flex",
        flexDirection: "column",
        gap: 2,
      }}
    >
      {error && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {error}
        </Alert>
      )}

      {sendResult && (
        <Alert severity="success">
          Transaction sent! Hash: {sendResult.tx_hash}
        </Alert>
      )}

      <WalletOverview
        mainAddress={mainAddress}
        balance={balance}
        isRefreshing={isRefreshing}
        onRefresh={refreshMoneroWallet}
      />

      <SyncProgress syncProgress={syncProgress} />

      <SendTransaction
        balance={balance}
        isSending={isSending}
        onSend={handleSendTransaction}
      />

      <TransactionHistory history={history} />
    </Box>
  );
}
