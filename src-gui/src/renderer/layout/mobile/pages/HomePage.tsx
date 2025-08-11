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
import SwipeableActionBottomSheet from "renderer/components/modal/SwipeableActionBottomSheet";
import TextIconButton from "renderer/components/buttons/TextIconButton";

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
  const [bottomSheetOpen, setBottomSheetOpen] = useState(false);

  const isLoading = balance === null;
  const hasTransactions =
    history && history.transactions && history.transactions.length > 0;

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
        <TextIconButton label="More" onClick={() => setBottomSheetOpen(true)}>
          <MoreVertIcon />
        </TextIconButton>
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
          <MobileTransactionHistory transactions={history!.transactions} />
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
      >
        <HelpOutlineIcon />
      </IconButton>

      {/* Mobile Action Bottom Sheet */}
      <SwipeableActionBottomSheet
        open={bottomSheetOpen}
        onOpen={() => setBottomSheetOpen(true)}
        onClose={() => setBottomSheetOpen(false)}
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
}: {
  transactions: TransactionInfo[];
}) {
  // Group transactions by date
  const transactionGroups = _(transactions)
    .groupBy((tx) => dayjs(tx.timestamp * 1000).format("YYYY-MM-DD"))
    .map((txs, dateKey) => ({
      date: dateKey,
      displayDate: dayjs(dateKey).format("MMMM D, YYYY"),
      transactions: _.orderBy(txs, ["timestamp"], ["desc"]),
    }))
    .orderBy(["date"], ["desc"])
    .take(3) // Show only the most recent 3 groups for mobile
    .value();

  return (
    <Stack spacing={3}>
      {transactionGroups.map((group) => (
        <Box key={group.date}>
          <Typography
            variant="body2"
            color="text.secondary"
            sx={{ mb: 1, fontSize: "0.75rem" }}
          >
            {group.displayDate}
          </Typography>
          <Stack spacing={1.5}>
            {group.transactions.slice(0, 3).map((tx) => (
              <React.Fragment key={tx.tx_hash}>
                <TransactionItem transaction={tx} />
                <Divider sx={{ opacity: 0.3 }} />
              </React.Fragment>
            ))}
          </Stack>
        </Box>
      ))}
      {transactions.length > 9 && (
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ textAlign: "center", fontStyle: "italic" }}
        >
          Showing recent transactions only
        </Typography>
      )}
    </Stack>
  );
}
