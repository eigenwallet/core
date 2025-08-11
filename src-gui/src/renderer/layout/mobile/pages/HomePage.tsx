import React, { useEffect, useState } from "react";
import {
  Box,
  Typography,
  IconButton,
  Stack,
  Card,
  CardContent,
  Button,
  useTheme,
  Divider,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import SettingsIcon from "@mui/icons-material/Settings";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import SwapHorizIcon from "@mui/icons-material/SwapHoriz";
import MoreHorizIcon from "@mui/icons-material/MoreHoriz";
import HelpOutlineIcon from "@mui/icons-material/HelpOutline";
import AppsIcon from "@mui/icons-material/Apps";
import { useAppSelector } from "store/hooks";
import {
  PiconeroAmount,
  FiatPiconeroAmount,
  SatsAmount,
} from "renderer/components/other/Units";
import TransactionItem from "renderer/components/pages/monero/components/TransactionItem";
import { TransactionInfo } from "models/tauriModel";
import { initializeMoneroWallet } from "renderer/rpc";
import dayjs from "dayjs";
import _ from "lodash";
import {
  WalletActionButtons,
  WalletOverview,
} from "renderer/components/pages/monero/components";
import MoneroWalletOverview from "renderer/components/features/wallet/MoneroWalletOverview.mobile";
import BitcoinWalletOverview from "renderer/components/features/wallet/BitcoinWalletOverview.mobile";
import ReceiveButton from "renderer/components/features/wallet/ReceiveButton.mobile";
import SendButton from "renderer/components/features/wallet/SendButton.mobile";
import DFXButton from "renderer/components/pages/monero/components/DFXWidget";
import { useNavigate } from "react-router-dom";
import TransactionDetailsBottomSheet from "renderer/components/modal/TransactionDetailsBottomSheet";

/**
 * Mobile HomePage - displays wallet overview with real balance and transaction data
 */
export default function HomePage() {
  const navigate = useNavigate();
  const theme = useTheme();
  const { balance, history, mainAddress, syncProgress } = useAppSelector(
    (state) => state.wallet.state,
  );
  const bitcoinBalance = useAppSelector((state) => state.rpc.state.balance);

  const isLoading = balance === null;
  const hasTransactions =
    history && history.transactions && history.transactions.length > 0;

  // Bottom sheet state
  const [selectedTransaction, setSelectedTransaction] = useState<TransactionInfo | null>(null);
  const [bottomSheetOpen, setBottomSheetOpen] = useState(false);

  const handleTransactionClick = (transaction: TransactionInfo) => {
    setSelectedTransaction(transaction);
    setBottomSheetOpen(true);
  };

  const handleBottomSheetClose = () => {
    setBottomSheetOpen(false);
    setSelectedTransaction(null);
  };

  useEffect(() => {
    initializeMoneroWallet();
  }, []);

  return (
    <Box
      sx={{
        p: 2,
        display: "flex",
        flexDirection: "column",
        gap: 3,
      }}
    >
      {/* Header */}
      <Stack direction="row" alignItems="center" spacing={2}>
        {/* Gradient avatar placeholder */}
        <Box
          sx={{
            width: 56,
            height: 56,
            borderRadius: "50%",
            background:
              "radial-gradient(circle at 30% 30%, #00FFC2 0%, #004F3B 100%)",
            border: `2px solid ${theme.palette.background.paper}`,
          }}
        />
        <Typography variant="h5" fontWeight={600} flexGrow={1}>
          Wallet 1
          <ExpandMoreIcon
            fontSize="small"
            sx={{ ml: 0.5, verticalAlign: "middle" }}
          />
        </Typography>
        <IconButton
          size="small"
          color="inherit"
          onClick={() => navigate("/settings", { viewTransition: true })}
        >
          <SettingsIcon />
        </IconButton>
      </Stack>

      {/* Balances */}
      <Stack spacing={1}>
        <MoneroWalletOverview balance={balance} />
        <BitcoinWalletOverview bitcoinBalance={bitcoinBalance} />
      </Stack>

      {/* Quick actions */}
      <Stack direction="row" spacing={2} justifyContent="center">
        <ReceiveButton address={mainAddress} />
        <SendButton balance={balance} />
        <DFXButton />
      </Stack>

      {/* Get Started */}
      <Box>
        <Typography variant="h6" gutterBottom>
          Get Started
        </Typography>
        <Stack direction="row" spacing={2} sx={{ overflowX: "auto", pb: 1 }}>
          <GetStartedCard
            gradient="linear-gradient(135deg, #5b5bff 0%, #b85bff 100%)"
            title="Begin Swaping"
            subtitle="Swap BTC → XMR"
          />
          <GetStartedCard
            gradient="linear-gradient(135deg, #ff8080 0%, #ff4d6d 100%)"
            title="Introduction"
            subtitle="What is eigenwalle"
          />
        </Stack>
      </Box>

      {/* Transactions */}
      <Box flexGrow={1}>
        <Typography variant="h6" gutterBottom>
          Transactions
        </Typography>
        {!hasTransactions ? (
          <Stack
            direction="row"
            spacing={1}
            alignItems="center"
            color="text.secondary"
            sx={{ opacity: 0.6 }}
          >
            <AppsIcon />
            <Typography variant="body2">
              {isLoading
                ? "Loading transactions..."
                : "Your transactions will show up here"}
            </Typography>
          </Stack>
        ) : (
          <MobileTransactionHistory 
            transactions={history!.transactions} 
            onViewAll={() => navigate("/transactions", { viewTransition: true })}
            onTransactionClick={handleTransactionClick}
          />
        )}
      </Box>

      {/* Floating help button */}
      <IconButton
        sx={{
          position: "fixed",
          bottom: 24,
          right: 24,
          width: 48,
          height: 48,
          borderRadius: "50%",
          backgroundColor:
            theme.palette.mode === "dark"
              ? "rgba(255,255,255,0.08)"
              : theme.palette.grey[200],
          backdropFilter: "blur(10px)",
          zIndex: theme.zIndex.fab,
        }}
        onClick={() => navigate("/feedback", { viewTransition: true })}
      >
        <HelpOutlineIcon />
      </IconButton>

      {/* Transaction Details Bottom Sheet */}
      <TransactionDetailsBottomSheet
        open={bottomSheetOpen}
        onClose={handleBottomSheetClose}
        transaction={selectedTransaction}
      />
    </Box>
  );
}

// Reusable Get Started card
function GetStartedCard({
  gradient,
  title,
  subtitle,
}: {
  gradient: string;
  title: string;
  subtitle: string;
}) {
  return (
    <Card
      sx={{
        minWidth: 220,
        borderRadius: 3,
        background: gradient,
        color: "#fff",
        flexShrink: 0,
      }}
    >
      <CardContent sx={{ p: 2, "&:last-child": { pb: 2 } }}>
        <Typography variant="subtitle1" fontWeight={600} gutterBottom>
          {title}
        </Typography>
        <Typography variant="caption" sx={{ opacity: 0.9 }}>
          {subtitle}
        </Typography>
      </CardContent>
    </Card>
  );
}

// Mobile-specific transaction history component
function MobileTransactionHistory({
  transactions,
  onViewAll,
  onTransactionClick,
}: {
  transactions: TransactionInfo[];
  onViewAll?: () => void;
  onTransactionClick?: (transaction: TransactionInfo) => void;
}) {
  // Get the 4 most recent transactions
  const recentTransactions = _.orderBy(transactions, ["timestamp"], ["desc"]).slice(0, 4);

  return (
    <Stack spacing={2}>
      <Stack spacing={1.5}>
        {recentTransactions.map((tx, index) => (
          <React.Fragment key={tx.tx_hash}>
            <TransactionItem 
              transaction={tx} 
              onClick={onTransactionClick ? () => onTransactionClick(tx) : undefined}
            />
            {index < recentTransactions.length - 1 && <Divider sx={{ opacity: 0.3 }} />}
          </React.Fragment>
        ))}
      </Stack>
      
      {transactions.length > 4 && onViewAll && (
        <Box sx={{ display: "flex", justifyContent: "center", mt: 2 }}>
          <Button
            variant="outlined"
            onClick={onViewAll}
            sx={{
              borderRadius: 20,
              px: 3,
              py: 1,
              textTransform: "none",
              fontWeight: 500,
            }}
          >
            View all
          </Button>
        </Box>
      )}
    </Stack>
  );
}
