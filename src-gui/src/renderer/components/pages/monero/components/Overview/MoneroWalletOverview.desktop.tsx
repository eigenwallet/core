import { Box, Typography, Card, LinearProgress } from "@mui/material";
import { useAppSelector } from "store/hooks";
import { PiconeroAmount } from "renderer/components/other/Units";
import { FiatPiconeroAmount } from "renderer/components/other/Units";
import StateIndicator from "renderer/components/pages/monero/components/StateIndicator";
import humanizeDuration from "humanize-duration";
import { GetMoneroSyncProgressResponse } from "models/tauriModel";

interface TimeEstimationResult {
  blocksLeft: number;
  hasDirectKnowledge: boolean;
  isStuck: boolean;
  formattedTimeRemaining: string | null;
}

const AVG_MONERO_BLOCK_SIZE_KB = 130;

function useSyncTimeEstimation(
  syncProgress: GetMoneroSyncProgressResponse | undefined,
): TimeEstimationResult | null {
  const poolStatus = useAppSelector((state) => state.pool.status);
  const restoreHeight = useAppSelector(
    (state) => state.wallet.state.restoreHeight,
  );

  if (restoreHeight == null || poolStatus == null) {
    return null;
  }

  const currentBlock = syncProgress?.current_block ?? 0;
  const targetBlock = syncProgress?.target_block ?? 0;
  const restoreBlock = restoreHeight.height;

  // For blocks before the restore height we only need to download the header
  const fastBlocksLeft =
    currentBlock < restoreBlock
      ? Math.max(0, Math.min(restoreBlock, targetBlock) - currentBlock)
      : 0;

  // For blocks after (or equal to) the restore height we need the full block data
  const fullBlocksLeft = Math.max(
    0,
    targetBlock - Math.max(currentBlock, restoreBlock),
  );

  const blocksLeft = fastBlocksLeft + fullBlocksLeft;

  // Treat blocksLeft = 1 as if we have no direct knowledge
  const hasDirectKnowledge = blocksLeft != null && blocksLeft > 1;

  const isStuck =
    poolStatus?.bandwidth_kb_per_sec != null &&
    poolStatus.bandwidth_kb_per_sec < 1;

  // A full blocks is 130kb, we assume a header is 2% of that
  const estimatedDownloadLeftSize =
    fullBlocksLeft * AVG_MONERO_BLOCK_SIZE_KB +
    (fastBlocksLeft * AVG_MONERO_BLOCK_SIZE_KB) / 50;

  const estimatedTimeRemaining =
    hasDirectKnowledge &&
    poolStatus?.bandwidth_kb_per_sec != null &&
    poolStatus.bandwidth_kb_per_sec > 0
      ? estimatedDownloadLeftSize / poolStatus.bandwidth_kb_per_sec
      : null;

  const formattedTimeRemaining = estimatedTimeRemaining
    ? humanizeDuration(estimatedTimeRemaining * 1000, {
        round: true,
        largest: 1,
      })
    : null;

  return {
    blocksLeft,
    hasDirectKnowledge,
    isStuck,
    formattedTimeRemaining,
  };
}

interface WalletOverviewProps {
  balance?: {
    unlocked_balance: string;
    total_balance: string;
  };
  syncProgress?: GetMoneroSyncProgressResponse;
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
  const timeEstimation = useSyncTimeEstimation(syncProgress);

  const pendingBalance =
    parseFloat(balance.total_balance) - parseFloat(balance.unlocked_balance);

  const isSyncing = syncProgress && syncProgress.progress_percentage < 100;

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

  return (
    <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
      {syncProgress && syncProgress.progress_percentage < 100 && (
        <LinearProgress
          value={
            timeEstimation?.hasDirectKnowledge ? progressPercentage : undefined
          }
          valueBuffer={
            // If the bandwidth is low, we may not be making progress
            // We don't show the buffer in this case
            timeEstimation?.hasDirectKnowledge && !timeEstimation?.isStuck
              ? progressPercentage
              : undefined
          }
          variant={
            timeEstimation?.hasDirectKnowledge ? "buffer" : "indeterminate"
          }
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
          alignItems: "stretch",
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
            justifyContent: "space-between",
            minHeight: "100%",
          }}
        >
          <StateIndicator
            color={isSyncing ? "primary" : "success"}
            pulsating={isSyncing}
          />
          <Box sx={{ textAlign: "right" }}>
            {poolStatus && isSyncing && !timeEstimation?.isStuck && (
              <>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ mt: 0.5, fontSize: "0.7rem", display: "block" }}
                >
                  {timeEstimation?.formattedTimeRemaining &&
                    !timeEstimation?.isStuck && (
                      <>
                        {timeEstimation.formattedTimeRemaining} left /{" "}
                        {poolStatus.bandwidth_kb_per_sec?.toFixed(1) ?? "0.0"}{" "}
                        KB/s
                      </>
                    )}
                </Typography>
              </>
            )}
            {isSyncing && timeEstimation?.hasDirectKnowledge && (
              <Typography variant="body2" color="text.secondary">
                {timeEstimation.blocksLeft?.toLocaleString()} blocks left
              </Typography>
            )}
          </Box>
        </Box>
      </Box>
    </Card>
  );
}
