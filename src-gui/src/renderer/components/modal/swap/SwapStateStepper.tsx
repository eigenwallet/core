import { Step, StepLabel, Stepper, Typography } from "@mui/material";
import { SwapState } from "models/storeModel";
import logger from "utils/logger";

export enum PathType {
  HAPPY_PATH = "happy path",
  RECOVERY_PATH = "recovery path",
}

export enum RecoveryScenario {
  GENERIC = "generic",
  FULL_REFUND = "full_refund",
  PARTIAL_REFUND = "partial_refund",
  COOPERATIVE_REDEEM = "cooperative_redeem",
}

type PathStep = [
  type: PathType,
  step: number,
  isError: boolean,
  scenario?: RecoveryScenario,
];

/**
 * Determines the current step in the swap process based on the previous and latest state.
 * @param prevState - The previous state of the swap process (null if it's the initial state)
 * @param latestState - The latest state of the swap process
 * @returns A tuple containing [PathType, activeStep, errorFlag]
 */
function getActiveStep(state: SwapState | null): PathStep | null {
  // In case we cannot infer a correct step from the state
  function fallbackStep(reason: string) {
    logger.error(
      `Unable to choose correct stepper type (reason: ${reason}, state: ${JSON.stringify(state)}`,
    );
    return null;
  }

  if (state === null) {
    return [PathType.HAPPY_PATH, 0, false];
  }

  const prevState = state.prev;
  const isReleased = state.curr.type === "Released";

  // If the swap is released we use the previous state to display the correct step
  const latestState = isReleased ? prevState : state.curr;

  // If the swap is released but we do not have a previous state we fallback
  if (latestState === null) {
    return fallbackStep(
      "Swap has been released but we do not have a previous state saved to display",
    );
  }

  // This should really never happen. For this statement to be true, the host has to submit a "Released" event twice
  if (latestState.type === "Released") {
    return fallbackStep(
      "Both the current and previous states are both of type 'Released'.",
    );
  }

  switch (latestState.type) {
    // Step 0: Initializing the swap
    // These states represent the very beginning of the swap process
    // No funds have been locked
    case "ReceivedQuote":
    case "WaitingForBtcDeposit":
    case "SwapSetupInflight":
      return null; // No funds have been locked yet

    // Step 1: Waiting for Bitcoin lock confirmation
    // Bitcoin has been locked, waiting for the counterparty to lock their XMR
    case "BtcLockTxInMempool":
      // We only display the first step as completed if the Bitcoin lock has been confirmed
      if (
        latestState.content.btc_lock_confirmations !== undefined &&
        latestState.content.btc_lock_confirmations > 0
      ) {
        return [PathType.HAPPY_PATH, 1, isReleased];
      }
      return [PathType.HAPPY_PATH, 0, isReleased];

    // Still Step 1: Both Bitcoin and XMR have been locked, waiting for Monero lock to be confirmed
    case "XmrLockTxInMempool":
      return [PathType.HAPPY_PATH, 1, isReleased];

    // Step 2: Waiting for encrypted signature to be sent to Alice
    // and for Alice to redeem the Bitcoin
    case "PreflightEncSig":
    case "InflightEncSig":
    case "EncryptedSignatureSent":
      return [PathType.HAPPY_PATH, 2, isReleased];

    // Step 3: Waiting for XMR redemption
    // Bitcoin has been redeemed by Alice, now waiting for us to redeem Monero
    case "WaitingForXmrConfirmationsBeforeRedeem":
    case "RedeemingMonero":
      return [PathType.HAPPY_PATH, 3, isReleased];

    // Step 4: Swap completed successfully
    // XMR redemption transaction is in mempool, swap is essentially complete
    case "XmrRedeemInMempool":
      return [PathType.HAPPY_PATH, 4, false];

    // Recovery Path States - Generic (early states before we know outcome)

    case "WaitingForCancelTimelockExpiration":
    case "CancelTimelockExpired":
      return [PathType.RECOVERY_PATH, 0, isReleased, RecoveryScenario.GENERIC];

    case "BtcCancelled":
      return [PathType.RECOVERY_PATH, 1, isReleased, RecoveryScenario.GENERIC];

    // Recovery Path States - Full Refund

    case "BtcRefundPublished":
    case "BtcEarlyRefundPublished":
      return [PathType.RECOVERY_PATH, 1, isReleased, RecoveryScenario.FULL_REFUND];

    case "BtcRefunded":
    case "BtcEarlyRefunded":
      return [PathType.RECOVERY_PATH, 2, false, RecoveryScenario.FULL_REFUND];

    // Recovery Path States - Partial Refund

    case "BtcPartialRefundPublished":
      return [PathType.RECOVERY_PATH, 1, isReleased, RecoveryScenario.PARTIAL_REFUND];

    case "BtcPartiallyRefunded":
      return [PathType.RECOVERY_PATH, 2, isReleased, RecoveryScenario.PARTIAL_REFUND];

    case "BtcAmnestyPublished":
      return [PathType.RECOVERY_PATH, 2, isReleased, RecoveryScenario.PARTIAL_REFUND];

    case "BtcAmnestyReceived":
      return [PathType.RECOVERY_PATH, 3, false, RecoveryScenario.PARTIAL_REFUND];

    case "BtcRefundBurnPublished":
      return [PathType.RECOVERY_PATH, 2, true, RecoveryScenario.PARTIAL_REFUND];

    case "BtcRefundBurnt":
      return [PathType.RECOVERY_PATH, 2, true, RecoveryScenario.PARTIAL_REFUND];

    case "BtcFinalAmnestyPublished":
      return [PathType.RECOVERY_PATH, 2, isReleased, RecoveryScenario.PARTIAL_REFUND];

    case "BtcFinalAmnestyConfirmed":
      return [PathType.RECOVERY_PATH, 3, false, RecoveryScenario.PARTIAL_REFUND];

    // Recovery Path States - Cooperative Redeem (after punishment)

    case "BtcPunished":
      return [PathType.RECOVERY_PATH, 1, true, RecoveryScenario.COOPERATIVE_REDEEM];

    case "AttemptingCooperativeRedeem":
    case "CooperativeRedeemAccepted":
      return [PathType.RECOVERY_PATH, 2, isReleased, RecoveryScenario.COOPERATIVE_REDEEM];

    case "CooperativeRedeemRejected":
      return [PathType.RECOVERY_PATH, 2, true, RecoveryScenario.COOPERATIVE_REDEEM];

    case "Resuming":
      return null;
    default:
      return fallbackStep("No step is assigned to the current state");
    // TODO: Make this guard work. It should force the compiler to check if we have covered all possible cases.
    // return exhaustiveGuard(latestState.type);
  }
}

function SwapStepper({
  steps,
  activeStep,
  error,
}: {
  steps: Array<{ label: string; duration: string }>;
  activeStep: number;
  error: boolean;
}) {
  return (
    <Stepper activeStep={activeStep}>
      {steps.map((step, index) => (
        <Step key={index}>
          <StepLabel
            optional={
              <Typography variant="caption">{step.duration}</Typography>
            }
            error={error && activeStep === index}
          >
            {step.label}
          </StepLabel>
        </Step>
      ))}
    </Stepper>
  );
}

const HAPPY_PATH_STEP_LABELS = [
  { label: "Locking your BTC", duration: "~12min" },
  { label: "They lock their XMR", duration: "~20min" },
  { label: "They redeem the BTC", duration: "~2min" },
  { label: "Redeeming your XMR", duration: "~1min" },
];

const RECOVERY_STEP_LABELS: Record<
  RecoveryScenario,
  Array<{ label: string; duration: string }>
> = {
  [RecoveryScenario.GENERIC]: [
    { label: "Cancelling swap", duration: "~1min" },
    { label: "Attempting recovery", duration: "" },
  ],
  [RecoveryScenario.FULL_REFUND]: [
    { label: "Cancelling swap", duration: "~1min" },
    { label: "Bitcoin refunded", duration: "~5min" },
  ],
  [RecoveryScenario.PARTIAL_REFUND]: [
    { label: "Cancelling swap", duration: "~1min" },
    { label: "Partial refund", duration: "~30min" },
    { label: "Remaining Bitcoin", duration: "~2min" },
  ],
  [RecoveryScenario.COOPERATIVE_REDEEM]: [
    { label: "Cancelling swap", duration: "~1min" },
    { label: "We have been punished", duration: "" },
    { label: "Attempting cooperative recovery", duration: "~2min" },
  ],
};

export default function SwapStateStepper({
  state,
}: {
  state: SwapState | null;
}) {
  const result = getActiveStep(state);

  if (result === null) {
    return null;
  }

  const [pathType, activeStep, error, scenario] = result;

  let steps: Array<{ label: string; duration: string }>;
  if (pathType === PathType.HAPPY_PATH) {
    steps = HAPPY_PATH_STEP_LABELS;
  } else {
    steps = RECOVERY_STEP_LABELS[scenario ?? RecoveryScenario.GENERIC];
  }

  return <SwapStepper steps={steps} activeStep={activeStep} error={error} />;
}
