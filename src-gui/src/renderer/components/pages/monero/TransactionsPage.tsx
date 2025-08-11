import { Box, Typography, IconButton } from "@mui/material";
import { ArrowBack } from "@mui/icons-material";
import { useNavigate } from "react-router-dom";
import { useAppSelector } from "store/hooks";
import TransactionHistory from "./components/TransactionHistory";

export default function TransactionsPage() {
  const navigate = useNavigate();
  const { history } = useAppSelector((state) => state.wallet.state);

  return (
    <Box>
      {/* Header with back button */}
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          mb: 3,
          gap: 2,
        }}
      >
        <IconButton
          onClick={() => navigate(-1)}
          sx={{
            backgroundColor: "grey.800",
            color: "white",
            "&:hover": {
              backgroundColor: "grey.700",
            },
          }}
        >
          <ArrowBack />
        </IconButton>
        <Typography variant="h4">Transactions</Typography>
      </Box>

      {/* Transactions content */}
      <Box
        sx={{
          display: "flex",
          gap: 2,
          flexDirection: "column",
          paddingBottom: 2,
        }}
      >
        <TransactionHistory history={history} showViewAllButton={false} />
      </Box>
    </Box>
  );
}