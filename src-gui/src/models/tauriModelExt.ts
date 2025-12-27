import { exhaustiveGuard } from "utils/typescriptUtils";
import {
  ApprovalRequest,
  ExpiredTimelocks,
  GetSwapInfoResponse,
  SelectMakerDetails,
  TauriBackgroundProgress,
  TauriSwapProgressEvent,
  SendMoneroDetails,
  ContextStatus,
  QuoteWithAddress,
  ExportBitcoinWalletResponse,
} from "./tauriModel";
import {
  ContextStatusType,
  ResultContextStatus,
  RPCSlice,
} from "store/features/rpcSlice";

export type TauriSwapProgressEventType = TauriSwapProgressEvent["type"];

// Wrapper for QuoteWithAddress with an optional approval request
// Approving that request will result in a swap being initiated with that maker
export type SortableQuoteWithAddress = {
  quote_with_address: QuoteWithAddress;
  approval: {
    request_id: string;
    expiration_ts: number;
  } | null;
};

export type TauriSwapProgressEventContent<
  T extends TauriSwapProgressEventType,
> = Extract<TauriSwapProgressEvent, { type: T }>["content"];

export type TauriSwapProgressEventExt<T extends TauriSwapProgressEventType> =
  Extract<TauriSwapProgressEvent, { type: T }>;

// See /swap/src/protocol/bob/state.rs#L57
// TODO: Replace this with a typeshare definition
export enum BobStateName {
  Started = "quote has been requested",
  SwapSetupCompleted = "execution setup done",
  BtcLockReadyToPublish = "btc lock ready to publish",
  BtcLocked = "btc is locked",
  XmrLockCandidateFound = "xmr lock transaction candidate found",
  XmrLockTransactionSeen = "xmr lock transaction seen",
  XmrLocked = "xmr is locked",
  EncSigSent = "encrypted signature is sent",
  BtcRedeemed = "btc is redeemed",
  CancelTimelockExpired = "cancel timelock is expired",
  BtcCancelled = "btc is cancelled",
  BtcRefundPublished = "btc refund is published",
  BtcPartialRefundPublished = "btc partial refund is published",
  BtcEarlyRefundPublished = "btc early refund is published",
  BtcRefunded = "btc is refunded",
  BtcEarlyRefunded = "btc is early refunded",
  BtcPartiallyRefunded = "btc is partially refunded",
  BtcAmnestyPublished = "btc amnesty is published",
  BtcAmnestyReceived = "btc amnesty is confirmed",
  XmrRedeemed = "xmr is redeemed",
  BtcPunished = "btc is punished",
  SafelyAborted = "safely aborted",
}

export function bobStateNameToHumanReadable(stateName: BobStateName): string {
  switch (stateName) {
    case BobStateName.Started:
      return "Started";
    case BobStateName.SwapSetupCompleted:
      return "Setup completed";
    case BobStateName.BtcLockReadyToPublish:
      return "Bitcoin lock ready to publish";
    case BobStateName.BtcLocked:
      return "Bitcoin locked";
    case BobStateName.XmrLockCandidateFound:
      return "Monero lock transaction found but not yet verified";
    case BobStateName.XmrLockTransactionSeen:
      return "Monero lock transaction waiting for confirmation";
    case BobStateName.XmrLocked:
      return "Monero locked and fully confirmed";
    case BobStateName.EncSigSent:
      return "Encrypted signature sent";
    case BobStateName.BtcRedeemed:
      return "Bitcoin redeemed";
    case BobStateName.CancelTimelockExpired:
      return "Cancel timelock expired";
    case BobStateName.BtcCancelled:
      return "Bitcoin cancelled";
    case BobStateName.BtcRefundPublished:
      return "Bitcoin refund published";
    case BobStateName.BtcEarlyRefundPublished:
      return "Bitcoin early refund published";
    case BobStateName.BtcPartialRefundPublished:
      return "Bitcoin partial refund published";
    case BobStateName.BtcAmnestyPublished:
      return "Bitcoin amnesty was granted";
    case BobStateName.BtcRefunded:
      return "Bitcoin refunded";
    case BobStateName.BtcEarlyRefunded:
      return "Bitcoin early refunded";
    case BobStateName.BtcPartiallyRefunded:
      return "Bitcoin partially refunded";
    case BobStateName.BtcAmnestyReceived:
      return "Bitcoin amnesty was received";
    case BobStateName.XmrRedeemed:
      return "Monero redeemed";
    case BobStateName.BtcPunished:
      return "Bitcoin punished";
    case BobStateName.SafelyAborted:
      return "Safely aborted";
    default:
      return exhaustiveGuard(stateName);
  }
}

// TODO: This is a temporary solution until we have a typeshare definition for BobStateName
export type GetSwapInfoResponseExt = GetSwapInfoResponse & {
  state_name: BobStateName;
};

export type TimelockNone = Extract<ExpiredTimelocks, { type: "None" }>;
export type TimelockCancel = Extract<ExpiredTimelocks, { type: "Cancel" }>;
export type TimelockPunish = Extract<ExpiredTimelocks, { type: "Punish" }>;

// This function returns the absolute block number of the timelock relative to the block the tx_lock was included in
export function getAbsoluteBlock(
  timelock: ExpiredTimelocks,
  cancelTimelock: number,
  punishTimelock: number,
): number {
  if (timelock.type === "None") {
    return cancelTimelock - timelock.content.blocks_left;
  }
  if (timelock.type === "Cancel") {
    return cancelTimelock + punishTimelock - timelock.content.blocks_left;
  }
  if (timelock.type === "Punish") {
    return cancelTimelock + punishTimelock;
  }

  // We match all cases
  return exhaustiveGuard(timelock);
}

export type BobStateNameRunningSwap = Exclude<
  BobStateName,
  | BobStateName.Started
  | BobStateName.SwapSetupCompleted
  | BobStateName.BtcRefunded
  | BobStateName.BtcPartiallyRefunded
  | BobStateName.BtcAmnestyPublished
  | BobStateName.BtcAmnestyReceived
  | BobStateName.BtcRefunded
  | BobStateName.BtcEarlyRefunded
  | BobStateName.BtcPunished
  | BobStateName.SafelyAborted
  | BobStateName.XmrRedeemed
>;

export type GetSwapInfoResponseExtRunningSwap = GetSwapInfoResponseExt & {
  state_name: BobStateNameRunningSwap;
};

export function isBobStateNameRunningSwap(
  state: BobStateName,
): state is BobStateNameRunningSwap {
  return ![
    BobStateName.Started,
    BobStateName.SwapSetupCompleted,
    BobStateName.BtcRefunded,
    BobStateName.BtcEarlyRefunded,
    BobStateName.BtcPartiallyRefunded,
    BobStateName.BtcAmnestyPublished,
    BobStateName.BtcAmnestyReceived,
    BobStateName.BtcPunished,
    BobStateName.SafelyAborted,
    BobStateName.XmrRedeemed,
  ].includes(state);
}

export type BobStateNamePossiblyCancellableSwap =
  | BobStateName.BtcLocked
  | BobStateName.XmrLockCandidateFound
  | BobStateName.XmrLockTransactionSeen
  | BobStateName.XmrLocked
  | BobStateName.EncSigSent
  | BobStateName.CancelTimelockExpired
  | BobStateName.BtcRefundPublished
  | BobStateName.BtcEarlyRefundPublished;

/**
Checks if a swap is in a state where it can possibly be cancelled

The following conditions must be met:
 - The bitcoin must be locked
 - The bitcoin must not be redeemed
 - The bitcoin must not be cancelled
 - The bitcoin must not be refunded
 - The bitcoin must not be punished
 - The bitcoin must not be early refunded

See: https://github.com/comit-network/xmr-btc-swap/blob/7023e75bb51ab26dff4c8fcccdc855d781ca4b15/swap/src/cli/cancel.rs#L16-L35
 */
export function isBobStateNamePossiblyCancellableSwap(
  state: BobStateName,
): state is BobStateNamePossiblyCancellableSwap {
  return [
    BobStateName.BtcLocked,
    BobStateName.XmrLockCandidateFound,
    BobStateName.XmrLockTransactionSeen,
    BobStateName.XmrLocked,
    BobStateName.EncSigSent,
    BobStateName.CancelTimelockExpired,
    BobStateName.BtcRefundPublished,
    BobStateName.BtcEarlyRefundPublished,
  ].includes(state);
}

export type BobStateNamePossiblyRefundableSwap =
  | BobStateName.BtcLocked
  | BobStateName.XmrLockCandidateFound
  | BobStateName.XmrLockTransactionSeen
  | BobStateName.XmrLocked
  | BobStateName.EncSigSent
  | BobStateName.CancelTimelockExpired
  | BobStateName.BtcCancelled
  | BobStateName.BtcRefundPublished
  | BobStateName.BtcEarlyRefundPublished;

/**
Checks if a swap is in a state where it can possibly be refunded (meaning it's not impossible)

The following conditions must be met:
 - The bitcoin must be locked
 - The bitcoin must not be redeemed
 - The bitcoin must not be refunded
 - The bitcoin must not be punished

See: https://github.com/comit-network/xmr-btc-swap/blob/7023e75bb51ab26dff4c8fcccdc855d781ca4b15/swap/src/cli/refund.rs#L16-L34
 */
export function isBobStateNamePossiblyRefundableSwap(
  state: BobStateName,
): state is BobStateNamePossiblyRefundableSwap {
  return [
    BobStateName.BtcLocked,
    BobStateName.XmrLockCandidateFound,
    BobStateName.XmrLockTransactionSeen,
    BobStateName.XmrLocked,
    BobStateName.EncSigSent,
    BobStateName.CancelTimelockExpired,
    BobStateName.BtcCancelled,
    BobStateName.BtcRefundPublished,
    BobStateName.BtcEarlyRefundPublished,
  ].includes(state);
}

/**
 * Type guard for GetSwapInfoResponseExt
 * "running" means the swap is in progress and not yet completed
 * If a swap is not "running" it means it is either completed or no Bitcoin have been locked yet
 * @param response
 */
export function isGetSwapInfoResponseRunningSwap(
  response: GetSwapInfoResponseExt,
): response is GetSwapInfoResponseExtRunningSwap {
  return isBobStateNameRunningSwap(response.state_name);
}

export type PendingApprovalRequest = ApprovalRequest & {
  content: Extract<ApprovalRequest["request_status"], { state: "Pending" }>;
};

export type PendingLockBitcoinApprovalRequest = ApprovalRequest & {
  request: Extract<ApprovalRequest["request"], { type: "LockBitcoin" }>;
  content: Extract<ApprovalRequest["request_status"], { state: "Pending" }>;
};

export type PendingSeedSelectionApprovalRequest = ApprovalRequest & {
  type: "SeedSelection";
  content: Extract<ApprovalRequest["request_status"], { state: "Pending" }>;
};

export function isPendingLockBitcoinApprovalEvent(
  event: ApprovalRequest,
): event is PendingLockBitcoinApprovalRequest {
  // Check if the request is a LockBitcoin request and is pending
  return (
    event.request.type === "LockBitcoin" &&
    event.request_status.state === "Pending"
  );
}

export function isPendingSeedSelectionApprovalEvent(
  event: ApprovalRequest,
): event is PendingSeedSelectionApprovalRequest {
  // Check if the request is a SeedSelection request and is pending
  return (
    event.request.type === "SeedSelection" &&
    event.request_status.state === "Pending"
  );
}

export function isPendingBackgroundProcess(
  process: TauriBackgroundProgress,
): process is TauriBackgroundProgress {
  return process.progress.type === "Pending";
}

export type TauriBitcoinSyncProgress = Extract<
  TauriBackgroundProgress,
  { componentName: "SyncingBitcoinWallet" }
>;

export function isBitcoinSyncProgress(
  progress: TauriBackgroundProgress,
): progress is TauriBitcoinSyncProgress {
  return progress.componentName === "SyncingBitcoinWallet";
}

export type PendingSelectMakerApprovalRequest = PendingApprovalRequest & {
  request: { type: "SelectMaker"; content: SelectMakerDetails };
};

export type PendingSendMoneroApprovalRequest = PendingApprovalRequest & {
  request: { type: "SendMonero"; content: SendMoneroDetails };
};

export type PendingPasswordApprovalRequest = PendingApprovalRequest & {
  request: { type: "PasswordRequest"; content: { wallet_path: string } };
};

export function isPendingSelectMakerApprovalEvent(
  event: ApprovalRequest,
): event is PendingSelectMakerApprovalRequest {
  // Check if the request is pending
  if (event.request_status.state !== "Pending") {
    return false;
  }

  // Check if the request is a SelectMaker request
  return event.request.type === "SelectMaker";
}

export function isPendingSendMoneroApprovalEvent(
  event: ApprovalRequest,
): event is PendingSendMoneroApprovalRequest {
  // Check if the request is pending
  if (event.request_status.state !== "Pending") {
    return false;
  }

  // Check if the request is a SendMonero request
  return event.request.type === "SendMonero";
}

export function isPendingPasswordApprovalEvent(
  event: ApprovalRequest,
): event is PendingPasswordApprovalRequest {
  // Check if the request is pending
  if (event.request_status.state !== "Pending") {
    return false;
  }

  // Check if the request is a PasswordRequest request
  return event.request.type === "PasswordRequest";
}

/**
 * Checks if any funds have been locked yet based on the swap progress event
 * Returns true for events where funds have been locked
 * @param event The TauriSwapProgressEvent to check
 * @returns True if funds have been locked, false otherwise
 */
export function haveFundsBeenLocked(
  event: TauriSwapProgressEvent | null | undefined,
): boolean {
  if (event === null || event === undefined) {
    return false;
  }

  switch (event.type) {
    case "Resuming":
    case "ReceivedQuote":
    case "WaitingForBtcDeposit":
    case "SwapSetupInflight":
      return false;
  }

  return true;
}

export function isContextFullyInitialized(
  status: ResultContextStatus | null,
): boolean {
  if (status == null || status.type === ContextStatusType.Error) {
    return false;
  }

  return (
    status.status.bitcoin_wallet_available &&
    status.status.monero_wallet_available &&
    status.status.database_available
  );
}

export function isContextWithBitcoinWallet(
  status: ContextStatus | null,
): boolean {
  return status?.bitcoin_wallet_available ?? false;
}

export function isContextWithMoneroWallet(
  status: ContextStatus | null,
): boolean {
  return status?.monero_wallet_available ?? false;
}

export type ExportBitcoinWalletResponseExt = ExportBitcoinWalletResponse & {
  wallet_descriptor: {
    descriptor: string;
  };
};

export function hasDescriptorProperty(
  response: ExportBitcoinWalletResponse,
): response is ExportBitcoinWalletResponseExt {
  return (
    typeof response.wallet_descriptor === "object" &&
    response.wallet_descriptor !== null &&
    "descriptor" in response.wallet_descriptor &&
    typeof (response.wallet_descriptor as { descriptor?: unknown })
      .descriptor === "string"
  );
}
