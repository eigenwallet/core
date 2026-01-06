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
import {
  BitcoinPartialRefundPublished,
  BitcoinPartiallyRefunded,
  BitcoinAmnestyPublished,
  BitcoinAmnestyReceived,
  BitcoinRefundBurnPublished,
  BitcoinRefundBurnt,
  BitcoinFinalAmnestyPublished,
  BitcoinFinalAmnestyConfirmed,
} from "./done/BitcoinPartialRefundPage";
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
import XmrLockTxInMempoolPage from "./in_progress/XmrLockInMempoolPage";
import { exhaustiveGuard } from "utils/typescriptUtils";
import DepositAndChooseOfferPage from "renderer/components/pages/swap/swap/init/deposit_and_choose_offer/DepositAndChooseOfferPage";
import InitPage from "./init/InitPage";
import PreflightEncSigPage from "./in_progress/PreflightEncSig";
import InflightEncSigPage from "./in_progress/InflightEncSigPage";

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
    case "VerifyingXmrLockTx":
      if (state.curr.type === "VerifyingXmrLockTx") {
        return (
          <CircularProgressWithSubtitle description="Validating Monero lock transaction..." />
        );
      }
      break;
    case "XmrLockTxInMempool":
      if (state.curr.type === "XmrLockTxInMempool") {
        return <XmrLockTxInMempoolPage {...state.curr.content} />;
      }
      break;
    case "PreflightEncSig":
      return <PreflightEncSigPage />;
    case "InflightEncSig":
      return <InflightEncSigPage />;
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
    case "WaitingForCancelTimelockExpiration":
      // TODO: Add better UI here!
      if (state.curr.type === "WaitingForCancelTimelockExpiration") {
        return (
          <CircularProgressWithSubtitle description="Waiting for cancel timelock expiration..." />
        );
      }
      break;
    case "CancelTimelockExpired":
      return <CancelTimelockExpiredPage />;
    case "BtcCancelled":
      return <BitcoinCancelledPage />;

    //// 8 different types of Bitcoin refund states we can be in
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
    case "BtcPartialRefundPublished":
      if (state.curr.type === "BtcPartialRefundPublished") {
        return <BitcoinPartialRefundPublished {...state.curr.content} />;
      }
      break;
    case "BtcPartiallyRefunded":
      if (state.curr.type === "BtcPartiallyRefunded") {
        return <BitcoinPartiallyRefunded {...state.curr.content} />;
      }
      break;
    case "BtcAmnestyPublished":
      if (state.curr.type === "BtcAmnestyPublished") {
        return <BitcoinAmnestyPublished {...state.curr.content} />;
      }
      break;
    case "BtcAmnestyReceived":
      if (state.curr.type === "BtcAmnestyReceived") {
        return <BitcoinAmnestyReceived {...state.curr.content} />;
      }
      break;

    //// 4 different types of refund burn / final amnesty states
    case "BtcRefundBurnPublished":
      if (state.curr.type === "BtcRefundBurnPublished") {
        return <BitcoinRefundBurnPublished {...state.curr.content} />;
      }
      break;
    case "BtcRefundBurnt":
      if (state.curr.type === "BtcRefundBurnt") {
        return <BitcoinRefundBurnt {...state.curr.content} />;
      }
      break;
    case "BtcFinalAmnestyPublished":
      if (state.curr.type === "BtcFinalAmnestyPublished") {
        return <BitcoinFinalAmnestyPublished {...state.curr.content} />;
      }
      break;
    case "BtcFinalAmnestyConfirmed":
      if (state.curr.type === "BtcFinalAmnestyConfirmed") {
        return <BitcoinFinalAmnestyConfirmed {...state.curr.content} />;
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
