import {
  Box,
  Typography,
  CircularProgress,
  Button,
  Card,
  CardContent,
  Divider,
  CardHeader,
  LinearProgress,
} from "@mui/material";
import { useEffect, useState } from "react";
import { useAppSelector } from "store/hooks";
import { PiconeroAmount } from "../../../other/Units";
import { FiatPiconeroAmount } from "../../../other/Units";
import StateIndicator from "./StateIndicator";

interface LinearProgressWithBufferProps {
  value: number;
  bufferMin?: number;
  bufferMax?: number;
  sx?: any;
}

function LinearProgressWithBuffer({
  value,
  bufferMin = 2,
  bufferMax = 5,
  sx,
}: LinearProgressWithBufferProps) {
  const [bufferProgressAddition, setBufferProgressAddition] = useState(
    Math.random() * (bufferMax - bufferMin) + bufferMin,
  );

  useEffect(() => {
    setBufferProgressAddition(
      Math.random() * (bufferMax - bufferMin) + bufferMin,
    );
  }, [value, bufferMin, bufferMax]);

  return (
    <LinearProgress
      value={value}
      valueBuffer={Math.min(value + bufferProgressAddition, 100)}
      variant="buffer"
      sx={sx}
    />
  );
}

interface WalletOverviewProps {
  balance?: {
    unlocked_balance: string;
    total_balance: string;
  };
  syncProgress?: {
    current_block: number;
    target_block: number;
    progress_percentage: number;
  };
}

// Component for displaying wallet address and balance
export default function WalletOverview({
  balance,
  syncProgress,
}: WalletOverviewProps) {
  const lowestCurrentBlock = useAppSelector(
    (state) => state.wallet.state.lowestCurrentBlock,
  );

  const poolStatus = useAppSelector((state) => state.pool.status);

  const pendingBalance =
    parseFloat(balance.total_balance) - parseFloat(balance.unlocked_balance);

  const isSyncing = syncProgress && syncProgress.progress_percentage < 100;
  const blocksLeft = syncProgress?.target_block - syncProgress?.current_block;

  // syncProgress.progress_percentage is not good to display
  // assuming we have an old wallet, eventually we will always only use the last few cm of the progress bar
  //
  // We calculate our own progress percentage
  // lowestCurrentBlock is the lowest block we have seen
  // currentBlock is the current block we are on (how war we've synced)
  // targetBlock is the target block we are syncing to
  //
  // The progressPercentage below is the progress on that path
  // If the lowestCurrentBlock is null, we fallback to the syncProgress.progress_percentage
  const progressPercentage =
    lowestCurrentBlock === null || !syncProgress
      ? syncProgress?.progress_percentage || 0
      : Math.max(
          0,
          Math.min(
            100,
            ((syncProgress.current_block - lowestCurrentBlock) /
              (syncProgress.target_block - lowestCurrentBlock)) *
              100,
          ),
        );

  return (
    <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
      {syncProgress && syncProgress.progress_percentage < 100 && (
        <LinearProgressWithBuffer
          value={progressPercentage}
          sx={{
            width: "100%",
            position: "absolute",
            top: 0,
            left: 0,
          }}
        />
      )}

      {/* Balance */}
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: "1.5fr 1fr max-content",
          rowGap: 0.5,
          columnGap: 2,
          mb: 1,
        }}
      >
        <Typography
          variant="body2"
          color="text.secondary"
          sx={{ mb: 1, gridColumn: "1", gridRow: "1" }}
        >
          Available Funds
        </Typography>
        <Typography variant="h4" sx={{ gridColumn: "1", gridRow: "2" }}>
          <PiconeroAmount
            amount={parseFloat(balance.unlocked_balance)}
            fixedPrecision={4}
            disableTooltip
          />
        </Typography>
        <Typography
          variant="body2"
          color="text.secondary"
          sx={{ gridColumn: "1", gridRow: "3" }}
        >
          <FiatPiconeroAmount amount={parseFloat(balance.unlocked_balance)} />
        </Typography>
        {pendingBalance > 0 && (
          <>
            <Typography
              variant="body2"
              color="warning"
              sx={{
                mb: 1,
                animation: "pulse 2s infinite",
                gridColumn: "2",
                gridRow: "1",
                alignSelf: "end",
              }}
            >
              Pending
            </Typography>

            <Typography
              variant="h5"
              sx={{ gridColumn: "2", gridRow: "2", alignSelf: "center" }}
            >
              <PiconeroAmount amount={pendingBalance} fixedPrecision={4} />
            </Typography>
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ gridColumn: "2", gridRow: "3" }}
            >
              <FiatPiconeroAmount amount={pendingBalance} />
            </Typography>
          </>
        )}

        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
          }}
        >
          <Box
            sx={{
              display: "flex",
              flexDirection: "row",
              alignItems: "center",
              justifyContent: "center",
              gap: 1,
            }}
          >
            <Typography variant="body2" color="text.secondary">
              {isSyncing
                ? `${(syncProgress.target_block - syncProgress.current_block).toLocaleString()} blocks left`
                : "synced"}
            </Typography>
            <StateIndicator
              color={isSyncing ? "primary" : "success"}
              pulsating={isSyncing}
            />
          </Box>
          {poolStatus && isSyncing && (
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ mt: 0.5, fontSize: "0.7rem" }}
            >
              {poolStatus.bandwidth_kb_per_sec.toFixed(1)} KB/s
            </Typography>
          )}
        </Box>
      </Box>
    </Card>
  );
}
