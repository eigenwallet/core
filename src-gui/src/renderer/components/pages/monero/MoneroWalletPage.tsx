import React, { useEffect, useState } from "react";
import { 
  Box, 
  Typography, 
  CircularProgress, 
  Alert, 
  TextField, 
  Button,
  Card,
  CardContent,
  Grid,
  InputAdornment,
  LinearProgress
} from "@mui/material";
import { Send as SendIcon, QrCodeScanner as QrCodeScannerIcon, Refresh as RefreshIcon } from "@mui/icons-material";
import { 
  GetMoneroBalanceResponse, 
  SendMoneroArgs,
  SendMoneroResponse,
  GetMoneroSyncProgressResponse
} from "models/tauriModel";
import { 
  getMoneroMainAddress, 
  getMoneroBalance, 
  sendMonero,
  getMoneroSyncProgress
} from "../../../rpc";
import ActionableMonospaceTextBox from "../../other/ActionableMonospaceTextBox";
import { PiconeroAmount } from "../../other/Units";

export default function MoneroWalletPage() {
  const [mainAddress, setMainAddress] = useState<string | null>(null);
  const [balance, setBalance] = useState<GetMoneroBalanceResponse | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  
  // Send form state
  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [sendResult, setSendResult] = useState<SendMoneroResponse | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  
  // Sync progress state
  const [syncProgress, setSyncProgress] = useState<GetMoneroSyncProgressResponse | null>(null);

  // Auto-refresh sync progress every 5 seconds if not fully synced
  useEffect(() => {
    if (!syncProgress || syncProgress.progress_percentage >= 100) {
      return;
    }

    const interval = setInterval(async () => {
      try {
        const response = await getMoneroSyncProgress();
        setSyncProgress(response);
      } catch (err) {
        console.error("Failed to fetch sync progress:", err);
      }
    }, 5000);

    return () => clearInterval(interval);
  }, [syncProgress]);

  useEffect(() => {
    const fetchData = async () => {
      try {
        setIsLoading(true);
        setError(null);
        
        const [addressResponse, balanceResponse, syncProgressResponse] = await Promise.all([
          getMoneroMainAddress(),
          getMoneroBalance(),
          getMoneroSyncProgress(),
        ]);
        
        setMainAddress(addressResponse.address);
        setBalance(balanceResponse);
        setSyncProgress(syncProgressResponse);
      } catch (err) {
        console.error("Failed to fetch Monero wallet data:", err);
        setError("Failed to fetch Monero wallet data.");
      } finally {
        setIsLoading(false);
      }
    };

    fetchData();
  }, []);

  const handleSend = async () => {
    if (!sendAddress || !sendAmount) return;
    
    try {
      setIsSending(true);
      setSendResult(null);
      
      const result = await sendMonero({
        address: sendAddress,
        amount: parseFloat(sendAmount) * 1e12, // Convert XMR to piconero
      });
      
      setSendResult(result);
      setSendAddress("");
      setSendAmount("");
      
      // Refresh balance after sending
      const newBalance = await getMoneroBalance();
      setBalance(newBalance);
      
    } catch (err) {
      console.error("Failed to send Monero:", err);
      setError("Failed to send Monero transaction.");
    } finally {
      setIsSending(false);
    }
  };

  const handleMaxAmount = () => {
    if (balance?.unlocked_balance) {
      // Convert piconero to XMR and leave some for fees
      const unlocked = parseFloat(balance.unlocked_balance);
      const maxAmount = (unlocked - 10000000000) / 1e12; // Subtract ~0.01 XMR for fees
      setSendAmount(Math.max(0, maxAmount).toString());
    }
  };

  const handleRefresh = async () => {
    try {
      setIsRefreshing(true);
      setError(null);
      setSendResult(null);
      
      const [addressResponse, balanceResponse, syncProgressResponse] = await Promise.all([
        getMoneroMainAddress(),
        getMoneroBalance(),
        getMoneroSyncProgress(),
      ]);
      
      setMainAddress(addressResponse.address);
      setBalance(balanceResponse);
      setSyncProgress(syncProgressResponse);
    } catch (err) {
      console.error("Failed to refresh Monero wallet data:", err);
      setError("Failed to refresh Monero wallet data.");
    } finally {
      setIsRefreshing(false);
    }
  };

  if (isLoading) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", mt: 4 }}>
        <CircularProgress />
      </Box>
    );
  }

  return (
    <Box sx={{ maxWidth: 800, mx: "auto", p: 2 }}>
      {error && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {error}
        </Alert>
      )}

      {sendResult && (
        <Alert severity="success" sx={{ mb: 2 }}>
          Transaction sent! Hash: {sendResult.tx_hash}
        </Alert>
      )}

      {/* Primary Address */}
      {mainAddress && (
        <Card sx={{ mb: 3 }}>
          <CardContent>
            <Typography variant="h6" gutterBottom>
              Primary Address
            </Typography>
            <ActionableMonospaceTextBox content={mainAddress} />
          </CardContent>
        </Card>
      )}

      {/* Balance */}
      {balance && (
        <Card sx={{ mb: 3 }}>
          <CardContent>
            <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 2 }}>
              <Typography variant="h6">
                Monero Balance
              </Typography>
              <Button
                variant="outlined"
                size="small"
                startIcon={isRefreshing ? <CircularProgress size={16} /> : <RefreshIcon />}
                onClick={handleRefresh}
                disabled={isRefreshing}
              >
                {isRefreshing ? "Refreshing..." : "Refresh"}
              </Button>
            </Box>
            <Box sx={{ display: "flex", gap: 4 }}>
              <Box>
                <Typography variant="body2" color="text.secondary">
                  Confirmed
                </Typography>
                <Typography variant="h5">
                  <PiconeroAmount amount={parseFloat(balance.total_balance) - parseFloat(balance.unlocked_balance)} /> XMR
                </Typography>
              </Box>
              <Box>
                <Typography variant="body2" color="text.secondary">
                  Unconfirmed (Available)
                </Typography>
                <Typography variant="h5" color="primary">
                  <PiconeroAmount amount={parseFloat(balance.unlocked_balance)} /> XMR
                </Typography>
              </Box>
            </Box>
          </CardContent>
        </Card>
      )}

      {/* Sync Progress */}
      {syncProgress && (
        <Card sx={{ mb: 3 }}>
          <CardContent>
            <Typography variant="h6" gutterBottom>
              Sync Progress
            </Typography>
            <Box sx={{ mb: 2 }}>
              <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 1 }}>
                <Typography variant="body2" color="text.secondary">
                  Block {syncProgress.current_block.toLocaleString()} of {syncProgress.target_block.toLocaleString()}
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
            </Box>
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
          </CardContent>
        </Card>
      )}

      {/* Send Transaction */}
      <Card>
        <CardContent>
          <Typography variant="h6" gutterBottom>
            Send Monero
          </Typography>
          
          <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
            <TextField
              fullWidth
              label="Pay to"
              placeholder="Monero address"
              value={sendAddress}
              onChange={(e) => setSendAddress(e.target.value)}
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">
                    <Button size="small" startIcon={<QrCodeScannerIcon />}>
                      Scan
                    </Button>
                  </InputAdornment>
                ),
              }}
            />
            
            <Box sx={{ display: "flex", gap: 1 }}>
              <TextField
                fullWidth
                label="Amount"
                placeholder="0.0"
                value={sendAmount}
                onChange={(e) => setSendAmount(e.target.value)}
                type="number"
                InputProps={{
                  endAdornment: <InputAdornment position="end">XMR</InputAdornment>,
                }}
              />
              <Button 
                variant="outlined" 
                onClick={handleMaxAmount}
                disabled={!balance?.unlocked_balance}
              >
                Max
              </Button>
            </Box>
            
            <Box sx={{ display: "flex", gap: 1, justifyContent: "flex-end" }}>
              <Button
                variant="outlined"
                onClick={() => {
                  setSendAddress("");
                  setSendAmount("");
                  setSendResult(null);
                }}
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
            </Box>
          </Box>
        </CardContent>
      </Card>
    </Box>
  );
}