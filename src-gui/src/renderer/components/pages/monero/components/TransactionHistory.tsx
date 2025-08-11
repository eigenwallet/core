import { Typography, Box, Button } from "@mui/material";
import { TransactionInfo } from "models/tauriModel";
import _ from "lodash";
import dayjs from "dayjs";
import TransactionItem from "./TransactionItem";
import { useNavigate } from "react-router-dom";

interface TransactionHistoryProps {
  history?: {
    transactions: TransactionInfo[];
  };
  limit?: number;
  showViewAllButton?: boolean;
}

interface TransactionGroup {
  date: string;
  displayDate: string;
  transactions: TransactionInfo[];
}

// Component for displaying transaction history
export default function TransactionHistory({
  history,
  limit,
  showViewAllButton = false,
}: TransactionHistoryProps) {
  const navigate = useNavigate();

  if (!history || !history.transactions || history.transactions.length === 0) {
    return <Typography variant="h5">Transactions</Typography>;
  }

  const transactions = history.transactions;

  // Apply limit if specified
  const limitedTransactions = limit ? transactions.slice(0, limit) : transactions;

  // Group transactions by date using dayjs and lodash
  const transactionGroups: TransactionGroup[] = _(limitedTransactions)
    .groupBy((tx) => dayjs(tx.timestamp * 1000).format("YYYY-MM-DD")) // Convert Unix timestamp to date string
    .map((txs, dateKey) => ({
      date: dateKey,
      displayDate: dayjs(dateKey).format("MMMM D, YYYY"), // Human-readable format
      transactions: _.orderBy(txs, ["timestamp"], ["desc"]), // Sort transactions within group by newest first
    }))
    .orderBy(["date"], ["desc"]) // Sort groups by newest date first
    .value();

  return (
    <Box>
      <Typography variant="h5" sx={{ mb: 2 }}>
        Transactions
      </Typography>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {transactionGroups.map((group) => (
          <Box key={group.date}>
            <Typography variant="body1" color="text.secondary" sx={{ mb: 1 }}>
              {group.displayDate}
            </Typography>
            <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {group.transactions.map((tx) => (
                <TransactionItem key={tx.tx_hash} transaction={tx} />
              ))}
            </Box>
          </Box>
        ))}
      </Box>
      
      {/* View all button */}
      {showViewAllButton && transactions.length > (limit || 0) && (
        <Box sx={{ display: "flex", justifyContent: "center", mt: 3 }}>
          <Button
            variant="contained"
            onClick={() => navigate("/transactions")}
            sx={{
              borderRadius: "20px",
              px: 4,
              py: 1,
              backgroundColor: "primary.main",
              "&:hover": {
                backgroundColor: "primary.dark",
              },
            }}
          >
            View all
          </Button>
        </Box>
      )}
    </Box>
  );
}
