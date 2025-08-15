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
import MoreVertIcon from "@mui/icons-material/MoreVert";
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
import SwipeableActionBottomSheet from "renderer/layout/mobile/components/SwipeableActionBottomSheet";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import TransactionDetailsBottomSheet from "renderer/layout/mobile/components/TransactionDetailsBottomSheet";
import AvatarWithProgress from "renderer/components/other/AvatarWithProgress";
import Header from "../components/Header";

/**
 * Mobile HomePage - displays wallet overview with real balance and transaction data
 */
export default function HomePage() {
  const navigate = useNavigate();
  const theme = useTheme();
  const { balance, history, mainAddress } = useAppSelector(
    (state) => state.wallet.state,
  );
  const bitcoinBalance = useAppSelector((state) => state.rpc.state.balance);

  const isLoading = true;
  const hasTransactions =
    history && history.transactions && history.transactions.length > 0;

  // Bottom sheet state
  const [selectedTransaction, setSelectedTransaction] = useState<TransactionInfo | null>(null);
  const [actionBottomSheetOpen, setActionBottomSheetOpen] = useState(false);
  const [transactionBottomSheetOpen, setTransactionBottomSheetOpen] = useState(false);

  const handleTransactionClick = (transaction: TransactionInfo) => {
    setSelectedTransaction(transaction);
    setTransactionBottomSheetOpen(true);
  };

  const handleBottomSheetClose = () => {
    setTransactionBottomSheetOpen(false);
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
      <Header />

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
        <TextIconButton label="More" onClick={() => setActionBottomSheetOpen(true)}>
          <MoreVertIcon />
        </TextIconButton>
      </Stack>

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

      {/* Mobile Action Bottom Sheet */}
      <SwipeableActionBottomSheet
        open={actionBottomSheetOpen}
        onOpen={() => setActionBottomSheetOpen(true)}
        onClose={() => setActionBottomSheetOpen(false)}
      />

      {/* Transaction Details Bottom Sheet */}
      <TransactionDetailsBottomSheet
        open={transactionBottomSheetOpen}
        onClose={handleBottomSheetClose}
        transaction={selectedTransaction}
      />
    </Box>
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
