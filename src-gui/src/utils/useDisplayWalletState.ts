import { TauriContextStatusEvent } from "models/tauriModel";
import { useAppSelector, usePendingBackgroundProcesses } from "store/hooks";
import { useState } from "react";

export function useDisplayWalletState() {
  const { syncProgress } = useAppSelector(
    (state) => state.wallet.state,
  );

  const backgroundProgress = usePendingBackgroundProcesses().map(([_, status]) => status);
  const contextStatus = useAppSelector((s) => s.rpc.status);

  let stateLabel = "loading";
  let isLoading = false;
  let isError = false;
  let contextProgress = 1;
  let backgroundProcessesProgress = 1;
  let moneroWalletProgress = syncProgress?.progress_percentage ?? 100;

  if (contextStatus === TauriContextStatusEvent.Available) {
    stateLabel = "ready";
    contextProgress = 1;
    isLoading = false;
  }

  if (contextStatus === TauriContextStatusEvent.Failed) {
    stateLabel = "The daemon has stopped unexpectedly";
    contextProgress = 0;
    isLoading = false;
    isError = true;
  }

  if(moneroWalletProgress !== 100) {
    stateLabel = "syncing monero wallet";
    isLoading = true;
  }

  if (backgroundProgress.length > 0) {
    isLoading = true;

    let  numberOfAdditionalProcesses = 0
      if(backgroundProgress.some(status => status.componentName === "ListSellers" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("listing sellers", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "OpeningDatabase" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("opening database", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "OpeningMoneroWallet" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("opening monero wallet", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "BackgroundRefund" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("refunding swap", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "SyncingBitcoinWallet" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("syncing bitcoin wallet", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "FullScanningBitcoinWallet" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("full scanning bitcoin wallet", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "OpeningBitcoinWallet" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("opening bitcoin wallet", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      if(backgroundProgress.some(status => status.componentName === "EstablishingTorCircuits" && status.progress.type === "Pending")) {
          stateLabel = backgroundProgressLabel("establishing tor circuits", numberOfAdditionalProcesses);
          numberOfAdditionalProcesses++;
      }

      backgroundProcessesProgress = (8-numberOfAdditionalProcesses)/8;
  }

  const progress = contextProgress*0.2 + backgroundProcessesProgress*0.4 + moneroWalletProgress/100*0.4;
  console.log(contextProgress, backgroundProcessesProgress, moneroWalletProgress, progress);
  return { stateLabel, progress, isLoading, isError };
}

function backgroundProgressLabel(label: string, additionalProcesses: number) {
    return additionalProcesses > 1 ? `${label} and other processes` : label;
}