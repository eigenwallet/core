import { SwapState } from "models/storeModel";
import { TauriSwapProgressEventType } from "models/tauriModelExt";
import CircularProgressWithSubtitle from "./components/CircularProgressWithSubtitle";
import BitcoinPunishedPage from "./done/BitcoinPunishedPage";
import {
  BitcoinRefundedPage,
  BitcoinEarlyRefundedPage,
  BitcoinEarlyRefundPublishedPage,
  BitcoinRefundPublishedPage,
} from "./done/BitcoinRefundedPage";
import XmrRedeemInMempoolPage from "./done/XmrRedeemInMempoolPage";
import ProcessExitedPage from "./exited/ProcessExitedPage";
import BitcoinCancelledPage from "./in_progress/BitcoinCancelledPage";
import BitcoinLockTxInMempoolPage from "./in_progress/BitcoinLockTxInMempoolPage";
import RedeemingMoneroPage from "./in_progress/RedeemingMoneroPage";
import CancelTimelockExpiredPage from "./in_progress/CancelTimelockExpiredPage";
import EncryptedSignatureSentPage from "./in_progress/EncryptedSignatureSentPage";
import ReceivedQuotePage from "./in_progress/ReceivedQuotePage";
import SwapSetupInflightPage from "./in_progress/SwapSetupInflightPage";
import WaitingForXmrConfirmationsBeforeRedeemPage from "./in_progress/WaitingForXmrConfirmationsBeforeRedeemPage";
import XmrLockedPage from "./in_progress/XmrLockedPage";
import XmrLockTxInMempoolPage from "./in_progress/XmrLockInMempoolPage";
import { exhaustiveGuard } from "utils/typescriptUtils";
import DepositAndChooseOfferPage from "renderer/components/pages/swap/swap/init/deposit_and_choose_offer/DepositAndChooseOfferPage";
import InitPage from "./init/InitPage";
import { Box } from "@mui/material";

export default function SwapStatePage({ state }: { state: SwapState | null }) {
  if (state === null) {
    return <InitPage />;
  }

  const type: TauriSwapProgressEventType = state.curr.type;

  switch (type) {
    case "Resuming":
      return <CircularProgressWithSubtitle description="Resuming swap..." />;
    case "ReceivedQuote":
      return <ReceivedQuotePage />;
    case "WaitingForBtcDeposit":
      // This double check is necessary for the typescript compiler to infer types
      if (state.curr.type === "WaitingForBtcDeposit") {
        return <DepositAndChooseOfferPage {...state.curr.content} />;
      }
      break;
    case "SwapSetupInflight":
      if (state.curr.type === "SwapSetupInflight") {
        return <SwapSetupInflightPage {...state.curr.content} />;
      }
      break;
    case "RetrievingMoneroBlockheight":
      return (
        <CircularProgressWithSubtitle description="Retrieving Monero blockheight..." />
      );
    case "BtcLockPublishInflight":
      return (
        <CircularProgressWithSubtitle description="Publishing Bitcoin lock transaction..." />
      );
    case "BtcLockTxInMempool":
      if (state.curr.type === "BtcLockTxInMempool") {
        return <BitcoinLockTxInMempoolPage {...state.curr.content} />;
      }
      break;
    case "XmrLockTxInMempool":
      if (state.curr.type === "XmrLockTxInMempool") {
        return <XmrLockTxInMempoolPage {...state.curr.content} />;
      }
      break;
    case "XmrLocked":
      return <XmrLockedPage />;
    case "EncryptedSignatureSent":
      return <EncryptedSignatureSentPage />;
    case "RedeemingMonero":
      return <RedeemingMoneroPage />;
    case "WaitingForXmrConfirmationsBeforeRedeem":
      if (state.curr.type === "WaitingForXmrConfirmationsBeforeRedeem") {
        return (
          <WaitingForXmrConfirmationsBeforeRedeemPage {...state.curr.content} />
        );
      }
      break;
    case "XmrRedeemInMempool":
      if (state.curr.type === "XmrRedeemInMempool") {
        return <XmrRedeemInMempoolPage {...state.curr.content} />;
      }
      break;
    case "CancelTimelockExpired":
      return <CancelTimelockExpiredPage />;
    case "BtcCancelled":
      return <BitcoinCancelledPage />;

    //// 4 different types of Bitcoin refund states we can be in
    case "BtcRefundPublished": // tx_refund has been published but has not been confirmed yet
      if (state.curr.type === "BtcRefundPublished") {
        return <BitcoinRefundPublishedPage {...state.curr.content} />;
      }
      break;
    case "BtcEarlyRefundPublished": // tx_early_refund has been published but has not been confirmed yet
      if (state.curr.type === "BtcEarlyRefundPublished") {
        return <BitcoinEarlyRefundPublishedPage {...state.curr.content} />;
      }
      break;
    case "BtcRefunded": // tx_refund has been confirmed
      if (state.curr.type === "BtcRefunded") {
        return <BitcoinRefundedPage {...state.curr.content} />;
      }
      break;
    case "BtcEarlyRefunded": // tx_early_refund has been confirmed
      if (state.curr.type === "BtcEarlyRefunded") {
        return <BitcoinEarlyRefundedPage {...state.curr.content} />;
      }
      break;

    //// 4 different types of Bitcoin punished states we can be in
    case "BtcPunished":
      if (state.curr.type === "BtcPunished") {
        return <BitcoinPunishedPage state={state.curr} />;
      }
      break;
    case "AttemptingCooperativeRedeem":
      return (
        <CircularProgressWithSubtitle description="Attempting to redeem the Monero with the help of the other party" />
      );
    case "CooperativeRedeemAccepted":
      return (
        <CircularProgressWithSubtitle description="The other party is cooperating with us to redeem the Monero..." />
      );
    case "CooperativeRedeemRejected":
      if (state.curr.type === "CooperativeRedeemRejected") {
        return <BitcoinPunishedPage state={state.curr} />;
      }
      break;
    case "Released":
      return <ProcessExitedPage prevState={state.prev} swapId={state.swapId} />;

    default:
      return exhaustiveGuard(type);
  }
}
