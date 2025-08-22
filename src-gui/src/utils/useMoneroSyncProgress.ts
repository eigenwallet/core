import { LinearProgressProps } from "@mui/material";
import humanizeDuration from "humanize-duration";
import { GetMoneroSyncProgressResponse } from "models/tauriModel";
import { useAppSelector } from "store/hooks";

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

export default function useMoneroSyncProgress() {
  const syncProgress = useAppSelector((state) => state.wallet.state.syncProgress);

  const lowestCurrentBlock = useAppSelector(
    (state) => state.wallet.state.lowestCurrentBlock,
  );

  const poolStatus = useAppSelector((state) => state.pool.status);
  const timeEstimation = useSyncTimeEstimation(syncProgress);

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

  const loadingBarPercentage = timeEstimation?.hasDirectKnowledge ? progressPercentage : undefined

  const loadingBarBuffer = timeEstimation?.hasDirectKnowledge && !timeEstimation?.isStuck
  ? progressPercentage
  : undefined;

  const loadingBarStyle: LinearProgressProps['variant'] = timeEstimation?.hasDirectKnowledge ? "buffer" : "indeterminate"

  let secondaryProgressInformation: string | undefined = undefined;
  if ( poolStatus && isSyncing && !timeEstimation?.isStuck && timeEstimation?.formattedTimeRemaining ) {
    secondaryProgressInformation = `${timeEstimation.formattedTimeRemaining} left / ${poolStatus.bandwidth_kb_per_sec?.toFixed(1) ?? "0.0"} KB/s`
  }

  let primaryProgressInformation: string | undefined = undefined;
  if ( isSyncing && timeEstimation?.hasDirectKnowledge) {
    primaryProgressInformation = `${timeEstimation.blocksLeft?.toLocaleString()} blocks left`
  }
  
  return {
    loadingBarStyle,
    loadingBarPercentage,
    loadingBarBuffer,
    primaryProgressInformation,
    secondaryProgressInformation,
    isSyncing,
  };
}