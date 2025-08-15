import React, { useState } from "react";
import {
  Box,
  IconButton,
  Typography,
  Stack,
  Divider,
} from "@mui/material";
import { useNavigate } from "react-router-dom";
import { ChevronLeft } from "@mui/icons-material";
import { useAppSelector } from "store/hooks";
import TransactionItem from "renderer/components/pages/monero/components/TransactionItem";
import { TransactionInfo } from "models/tauriModel";
import dayjs from "dayjs";
import _ from "lodash";
import TransactionDetailsBottomSheet from "renderer/layout/mobile/components/TransactionDetailsBottomSheet";

export default function TransactionsPage() {
  const navigate = useNavigate();
  const { history } = useAppSelector((state) => state.wallet.state);
  
  const hasTransactions = history && history.transactions && history.transactions.length > 0;

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

  return (
    <Box>
      {/* Header with back button */}
      <Box 
        sx={{ 
          px: 2, 
          pt: 3, 
          display: "flex", 
          alignItems: "center", 
          gap: 1, 
          position: "sticky", 
          top: 0, 
          backgroundColor: "background.paper", 
          zIndex: 1 
        }}
      >
        <IconButton onClick={() => navigate("/", { viewTransition: true })}>
          <ChevronLeft />
        </IconButton>
        <Typography variant="h5">Transactions</Typography>
      </Box>
      
      {/* Content */}
      <Box sx={{ p: 2 }}>
        {!hasTransactions ? (
          <Typography variant="body2" color="text.secondary" sx={{ textAlign: "center", mt: 4 }}>
            No transactions found
          </Typography>
        ) : (
          <AllTransactionHistory 
            transactions={history!.transactions}
            onTransactionClick={handleTransactionClick}
          />
        )}
      </Box>

      {/* Transaction Details Bottom Sheet */}
      <TransactionDetailsBottomSheet
        open={bottomSheetOpen}
        onClose={handleBottomSheetClose}
        transaction={selectedTransaction}
      />
    </Box>
  );
}

// Component to display all transactions grouped by date
function AllTransactionHistory({
  transactions,
  onTransactionClick,
}: {
  transactions: TransactionInfo[];
  onTransactionClick?: (transaction: TransactionInfo) => void;
}) {
  
  const transactionGroups = _(transactions)
    .groupBy((tx) => dayjs(tx.timestamp * 1000).format("YYYY-MM-DD"))
    .map((txs, dateKey) => ({
      date: dateKey,
      displayDate: dayjs(dateKey).format("MMMM D, YYYY"),
      transactions: _.orderBy(txs, ["timestamp"], ["desc"]),
    }))
    .orderBy(["date"], ["desc"])
    .value();

  return (
    <Stack spacing={3}>
      {transactionGroups.map((group) => (
        <Box key={group.date}>
          <Typography
            variant="body2"
            color="text.secondary"
            sx={{ mb: 2, fontSize: "0.75rem", fontWeight: 600 }}
          >
            {group.displayDate}
          </Typography>
          <Stack spacing={1.5}>
            {group.transactions.map((tx, index) => (
              <React.Fragment key={tx.tx_hash}>
                <TransactionItem 
                  transaction={tx}
                  onClick={onTransactionClick ? () => onTransactionClick(tx) : undefined}
                />
                {index < group.transactions.length - 1 && <Divider sx={{ opacity: 0.3 }} />}
              </React.Fragment>
            ))}
          </Stack>
        </Box>
      ))}
    </Stack>
  );
}