import { Box, Typography, Card, LinearProgress } from "@mui/material";
import { useAppSelector } from "store/hooks";
import { PiconeroAmount } from "../../../other/Units";
import { FiatPiconeroAmount } from "../../../other/Units";
import StateIndicator from "./StateIndicator";

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
  
  // Treat blocksLeft = 1 as if we have no direct knowledge
  const hasDirectKnowledge = blocksLeft != null && blocksLeft > 1;

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
      : syncProgress.target_block === lowestCurrentBlock
      ? 100 // Fully synced when target equals lowest current block
      : Math.max(
          0,
          Math.min(
            100,
            ((syncProgress.current_block - lowestCurrentBlock) /
              (syncProgress.target_block - lowestCurrentBlock)) *
              100,
          ),
        );

  const isStuck = poolStatus?.bandwidth_kb_per_sec != null && poolStatus.bandwidth_kb_per_sec < 0.01;

  // Calculate estimated time remaining for sync
  const formatTimeRemaining = (seconds: number): string => {
    if (seconds < 60) return `${Math.round(seconds)}s`;
    if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
    if (seconds < 86400) return `${Math.round(seconds / 3600)}h`;
    return `${Math.round(seconds / 86400)}d`;
  };

  const estimatedTimeRemaining =
    hasDirectKnowledge && poolStatus?.bandwidth_kb_per_sec != null && poolStatus.bandwidth_kb_per_sec > 0
      ? (blocksLeft * 130) / poolStatus.bandwidth_kb_per_sec // blocks * 130kb / kb_per_sec = seconds
      : null;

  return (
    <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
      {syncProgress && syncProgress.progress_percentage < 100 && (
        <LinearProgress
          value={hasDirectKnowledge ? progressPercentage : undefined}
          valueBuffer={
            // If the bandwidth is low, we may not be making progress
            // We don't show the buffer in this case
            hasDirectKnowledge && !isStuck ? progressPercentage : undefined
          }
          variant={hasDirectKnowledge ? "buffer" : "indeterminate"}
          sx={{
            position: "absolute",
            top: 0,
            left: 0,
            width: "100%",
          }}
        />
      )}

      {/* Balance */}
      <Box
        sx={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "flex-start",
          mb: 1,
        }}
      >
        {/* Left side content */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            gap: 4,
          }}
        >
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: 0.5,
            }}
          >
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              Available Funds
            </Typography>
            <Typography variant="h4">
              <PiconeroAmount
                amount={parseFloat(balance.unlocked_balance)}
                fixedPrecision={4}
                disableTooltip
              />
            </Typography>
            <Typography variant="body2" color="text.secondary">
              <FiatPiconeroAmount
                amount={parseFloat(balance.unlocked_balance)}
              />
            </Typography>
          </Box>
          {pendingBalance > 0 && (
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                gap: 0.5,
              }}
            >
              <Typography
                variant="body2"
                color="warning"
                sx={{
                  mb: 1,
                  animation: "pulse 2s infinite",
                }}
              >
                Pending
              </Typography>
              <Typography variant="h5">
                <PiconeroAmount amount={pendingBalance} fixedPrecision={4} />
              </Typography>
              <Typography variant="body2" color="text.secondary">
                <FiatPiconeroAmount amount={pendingBalance} />
              </Typography>
            </Box>
          )}
        </Box>

        {/* Right side - simple approach */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
            gap: 2,
          }}
        >
          <StateIndicator
            color={isSyncing ? "primary" : "success"}
            pulsating={isSyncing}
          />
          <Box sx={{ textAlign: "right" }}>
            {isSyncing && hasDirectKnowledge && (
              <Typography variant="body2" color="text.secondary">
                {blocksLeft?.toLocaleString()} blocks left
              </Typography>
            )}
            {poolStatus && isSyncing && !isStuck && (
              <>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ mt: 0.5, fontSize: "0.7rem", display: "block" }}
                >
                  {estimatedTimeRemaining && !isStuck && (
                    <>{formatTimeRemaining(estimatedTimeRemaining)} left</>
                  )} / {poolStatus.bandwidth_kb_per_sec?.toFixed(1) ?? '0.0'} KB/s
                </Typography>
              </>
            )}
          </Box>
        </Box>
      </Box>
    </Card>
  );
}
