import {
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  TextField,
  Typography,
  Button,
  Box,
  Card,
  CardContent,
  Breadcrumbs,
} from "@mui/material";
import NewPasswordInput from "renderer/components/other/NewPasswordInput";
import { useState, useEffect, useReducer, useRef } from "react";
import { usePendingSeedSelectionApproval } from "store/hooks";
import { resolveApproval, checkSeed } from "renderer/rpc";
import { SeedChoice } from "models/tauriModel";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import AddIcon from "@mui/icons-material/Add";
import RefreshIcon from "@mui/icons-material/Refresh";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import BackupSeedStep from "./BackupSeedStep";
import OpenWalletStep from "./OpenWalletStep";
import NameLocationStep from "./NameLocationStep";
import SeedPhraseInput from "./SeedPhraseInput";

type WalletMode = "RandomSeed" | "FromSeed" | "FromWalletPath";

type StepId =
  | "chooseMode"
  | "randomPassword"
  | "nameLocation"
  | "backupSeed"
  | "seedPhrase"
  | "storage"
  | "openFile";

/// What the primary (bottom-right) button does on a step.
type PrimaryActionKind =
  | "next"
  | "createWallet"
  | "restoreWallet"
  | "openWallet"
  | "finish";

interface Step {
  id: StepId;
  label: string;
  action: PrimaryActionKind;
}

// Single source of truth for each mode's wizard steps. Labels, content, the
// primary action, and navigation are all derived from this table, so a flow
// only ever changes here.
const FLOWS: Record<WalletMode, Step[]> = {
  RandomSeed: [
    { id: "chooseMode", label: "Choose wallet", action: "next" },
    { id: "randomPassword", label: "Set password", action: "next" },
    { id: "nameLocation", label: "Name & location", action: "createWallet" },
    { id: "backupSeed", label: "Back up seed", action: "finish" },
  ],
  FromSeed: [
    { id: "chooseMode", label: "Choose wallet", action: "next" },
    { id: "seedPhrase", label: "Enter seed phrase", action: "next" },
    { id: "storage", label: "Wallet storage", action: "next" },
    { id: "nameLocation", label: "Name & location", action: "restoreWallet" },
  ],
  FromWalletPath: [
    { id: "chooseMode", label: "Choose wallet", action: "next" },
    { id: "openFile", label: "Open wallet file", action: "openWallet" },
  ],
};

const PRIMARY_LABELS: Record<PrimaryActionKind, string> = {
  next: "Continue",
  createWallet: "Create wallet",
  restoreWallet: "Restore wallet",
  openWallet: "Open wallet",
  finish: "Finish",
};

function stepIndex(mode: WalletMode, step: StepId): number {
  return FLOWS[mode].findIndex((s) => s.id === step);
}

// The wizard is a state machine. `editing` navigates the FLOWS table of the
// selected mode. `backingUp` is entered once a new wallet has been created:
// the approval is already resolved, but the dialog stays up until the user has
// recorded the seed. `finished` unmounts the dialog. Only `editing` carries a
// mode/step, so out-of-range steps or "backup while restoring" cannot be
// represented.
type WizardState =
  | { phase: "editing"; mode: WalletMode; step: StepId }
  | { phase: "backingUp" }
  | { phase: "finished" };

type WizardEvent =
  | { type: "reset"; mode: WalletMode }
  | { type: "selectMode"; mode: WalletMode }
  | { type: "next" }
  | { type: "back" }
  | { type: "navigateBackTo"; step: StepId }
  | { type: "walletCreationStarted" }
  | { type: "walletCreationFailed" }
  | { type: "finish" };

// Every transition is guarded: an event that does not apply to the current
// state is a no-op instead of producing an invalid state.
function wizardReducer(state: WizardState, event: WizardEvent): WizardState {
  switch (event.type) {
    case "reset":
      return { phase: "editing", mode: event.mode, step: "chooseMode" };
    case "walletCreationStarted":
      return state.phase === "editing" ? { phase: "backingUp" } : state;
    case "walletCreationFailed":
      // Creation only starts from the RandomSeed name & location step, so
      // return there for the user to correct the input and retry.
      return state.phase === "backingUp"
        ? { phase: "editing", mode: "RandomSeed", step: "nameLocation" }
        : state;
    case "finish":
      return state.phase === "backingUp" ? { phase: "finished" } : state;
  }

  // The remaining events navigate within the editing phase.
  if (state.phase !== "editing") return state;
  const flow = FLOWS[state.mode];
  const index = stepIndex(state.mode, state.step);

  switch (event.type) {
    case "selectMode":
      // First click selects a mode, clicking the selected mode again advances.
      if (state.step !== "chooseMode") return state;
      return state.mode === event.mode
        ? { ...state, step: flow[1].id }
        : { ...state, mode: event.mode };
    case "next":
      return index < flow.length - 1
        ? { ...state, step: flow[index + 1].id }
        : state;
    case "back":
      return index > 0 ? { ...state, step: flow[index - 1].id } : state;
    case "navigateBackTo": {
      const target = stepIndex(state.mode, event.step);
      return target !== -1 && target < index
        ? { ...state, step: event.step }
        : state;
    }
  }
}

/**
 * Parses a block height input string and returns a number if valid, 0 for empty string, or false if invalid.
 *
 * Handles edge cases:
 * - Empty string: returns 0 (valid, means "scan from beginning")
 * - Whitespace-only: returns false (invalid)
 * - Non-numeric characters: returns false (invalid)
 * - Negative numbers: returns false (invalid)
 * - Zero: returns 0 (valid)
 * - Positive integers: returns the number (valid)
 */
function parseBlockHeightInput(blockheightInput: string): number | false {
  if (blockheightInput.length === 0) {
    return 0;
  }

  if (!/^\d+$/.test(blockheightInput)) {
    return false;
  }

  const blockheightNum = parseInt(blockheightInput, 10);

  if (
    blockheightInput === "0" ||
    (blockheightNum && !Number.isNaN(blockheightNum) && blockheightNum >= 0)
  ) {
    return blockheightNum;
  }

  return false;
}

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [wizard, dispatch] = useReducer(wizardReducer, {
    phase: "editing",
    mode: "RandomSeed",
    step: "chooseMode",
  } as WizardState);
  const [customSeed, setCustomSeed] = useState<string>("");
  const [blockheightInput, setBlockheightInput] = useState<string>("");
  const [asyncSeedValidation, setAsyncSeedValidation] =
    useState<boolean>(false);
  const [password, setPassword] = useState<string>("");
  const [isPasswordValid, setIsPasswordValid] = useState<boolean>(true);
  const [walletPath, setWalletPath] = useState<string>("");
  const [name, setName] = useState<string>("");
  const [directory, setDirectory] = useState<string>("");
  const [backupConfirmed, setBackupConfirmed] = useState<boolean>(false);

  const approval = pendingApprovals[0];
  const content =
    approval?.request?.type === "SeedSelection"
      ? approval.request.content
      : undefined;
  const recentWallets = content?.recent_wallets ?? [];

  // Reset the wizard whenever a new seed-selection approval arrives (e.g. after
  // the user cancels a password prompt and is asked to choose again).
  const lastRequestIdRef = useRef<string | null>(null);
  useEffect(() => {
    const requestId = approval?.request_id;
    if (!requestId || requestId === lastRequestIdRef.current) return;
    lastRequestIdRef.current = requestId;

    // Default to opening a recent wallet when one exists, otherwise create.
    dispatch({
      type: "reset",
      mode: recentWallets.length > 0 ? "FromWalletPath" : "RandomSeed",
    });
    setBackupConfirmed(false);
    setName("");
    setDirectory(content?.default_wallet_directory ?? "");
    if (recentWallets.length > 0) {
      setWalletPath(recentWallets[0]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset only on a new approval id
  }, [approval?.request_id]);

  // Only run async validation when in "FromSeed" mode with content
  const needsSeedValidation =
    wizard.phase === "editing" &&
    wizard.mode === "FromSeed" &&
    customSeed.trim();

  useEffect(() => {
    if (!needsSeedValidation) return;

    checkSeed(customSeed.trim())
      .then(setAsyncSeedValidation)
      .catch(() => setAsyncSeedValidation(false));
  }, [customSeed, needsSeedValidation]);

  const isSeedValid = needsSeedValidation && asyncSeedValidation;
  const hasBlockheightInput = blockheightInput.length > 0;
  const isBlockheightValid = parseBlockHeightInput(blockheightInput) !== false;
  const isBlockheightInvalid =
    hasBlockheightInput && isBlockheightValid === false;

  const buildSeedChoice = (mode: WalletMode): SeedChoice => {
    switch (mode) {
      case "RandomSeed":
        return { type: "RandomSeed", content: { password, name, directory } };
      case "FromSeed": {
        const parsedBlockHeight = parseBlockHeightInput(blockheightInput);
        if (parsedBlockHeight === false) {
          throw new Error("Invalid blockheight");
        }
        return {
          type: "FromSeed",
          content: {
            seed: customSeed,
            password,
            restore_height: parsedBlockHeight,
            name,
            directory,
          },
        };
      }
      case "FromWalletPath":
        return { type: "FromWalletPath", content: { wallet_path: walletPath } };
    }
  };

  const resolve = async (seedChoice: SeedChoice) => {
    if (!approval)
      throw new Error("No approval request found for seed selection");
    await resolveApproval<SeedChoice>(approval.request_id, seedChoice);
  };

  // Creating transitions to the backup phase *before* resolving, so the dialog
  // stays mounted once the approval clears; on failure we roll back.
  const createWallet = async () => {
    dispatch({ type: "walletCreationStarted" });
    try {
      await resolve(buildSeedChoice("RandomSeed"));
    } catch (e) {
      dispatch({ type: "walletCreationFailed" });
      throw e;
    }
  };

  if (wizard.phase === "finished") return null;
  if (wizard.phase === "editing" && !approval) return null;

  // The backup phase is only reachable from the RandomSeed flow, whose last
  // step records the seed.
  const mode = wizard.phase === "editing" ? wizard.mode : "RandomSeed";
  const step = wizard.phase === "editing" ? wizard.step : "backupSeed";
  const flow = FLOWS[mode];
  const index = stepIndex(mode, step);

  // Whether the current step's input is complete enough to act on.
  const stepValid: Record<StepId, boolean> = {
    chooseMode: true,
    randomPassword: isPasswordValid,
    seedPhrase:
      customSeed.trim().length > 0 && !!isSeedValid && !isBlockheightInvalid,
    storage: isPasswordValid,
    nameLocation: name.trim().length > 0 && directory.trim().length > 0,
    openFile: walletPath.length > 0,
    backupSeed: backupConfirmed,
  };

  const primaryHandlers: Record<PrimaryActionKind, () => void | Promise<void>> =
    {
      next: () => dispatch({ type: "next" }),
      finish: () => dispatch({ type: "finish" }),
      createWallet,
      restoreWallet: () => resolve(buildSeedChoice("FromSeed")),
      openWallet: () => resolve(buildSeedChoice("FromWalletPath")),
    };

  return (
    <Dialog
      open={true}
      maxWidth="sm"
      fullWidth
      sx={{ "& .MuiDialog-paper": { height: "min(32rem, 80vh)" } }}
      BackdropProps={{
        sx: {
          backdropFilter: "blur(8px)",
          backgroundColor: "rgba(0, 0, 0, 0.5)",
        },
      }}
    >
      <DialogTitle sx={{ pb: 1 }}>
        <Breadcrumbs
          aria-label="wallet setup steps"
          sx={{
            // Keep the trail on one line, fading out the overflow rather than
            // wrapping to a second line.
            overflow: "hidden",
            "& .MuiBreadcrumbs-ol": { flexWrap: "nowrap" },
            "& .MuiBreadcrumbs-li": { whiteSpace: "nowrap" },
            maskImage:
              "linear-gradient(to right, black calc(100% - 32px), transparent)",
            WebkitMaskImage:
              "linear-gradient(to right, black calc(100% - 32px), transparent)",
          }}
        >
          {flow.map((s, i) => {
            const crumbLabel = i === 0 ? "Set up your wallet" : s.label;
            // Past crumbs link back; upcoming crumbs preview muted; navigation
            // is disabled once a wallet has been created.
            const navigable = i < index && wizard.phase === "editing";
            const upcoming = i > index;

            return navigable ? (
              <Typography
                key={s.id}
                color="primary"
                onClick={() => dispatch({ type: "navigateBackTo", step: s.id })}
                sx={{
                  cursor: "pointer",
                  "&:hover": { textDecoration: "underline" },
                }}
              >
                {crumbLabel}
              </Typography>
            ) : (
              <Typography
                key={s.id}
                color={upcoming ? "text.disabled" : "text.primary"}
              >
                {crumbLabel}
              </Typography>
            );
          })}
        </Breadcrumbs>
      </DialogTitle>
      <DialogContent
        sx={{ display: "flex", flexDirection: "column", gap: 3, pt: 1 }}
      >
        {step === "chooseMode" && (
          <Box sx={{ display: "flex", flexDirection: "row", gap: 2 }}>
            <ModeCard
              selected={mode === "FromWalletPath"}
              icon={
                <FolderOpenIcon
                  sx={{ fontSize: 32, color: "text.secondary" }}
                />
              }
              label="Open wallet file"
              onClick={() =>
                dispatch({ type: "selectMode", mode: "FromWalletPath" })
              }
            />
            <ModeCard
              selected={mode === "RandomSeed"}
              icon={<AddIcon sx={{ fontSize: 32, color: "text.secondary" }} />}
              label="Create new wallet"
              onClick={() =>
                dispatch({ type: "selectMode", mode: "RandomSeed" })
              }
            />
            <ModeCard
              selected={mode === "FromSeed"}
              icon={
                <RefreshIcon sx={{ fontSize: 32, color: "text.secondary" }} />
              }
              label="Restore from seed"
              onClick={() => dispatch({ type: "selectMode", mode: "FromSeed" })}
            />
          </Box>
        )}

        {step === "randomPassword" && (
          <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            <NewPasswordInput
              password={password}
              setPassword={setPassword}
              isPasswordValid={isPasswordValid}
              setIsPasswordValid={setIsPasswordValid}
            />
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ textAlign: "center" }}
            >
              A new wallet with a random seed phrase will be generated. You will
              record the seed in the final step.
            </Typography>
          </Box>
        )}

        {step === "seedPhrase" && (
          <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            <Typography variant="body2" color="text.secondary">
              Enter the 25-word Monero seed phrase of the wallet you want to
              restore. Setting the restore height to around when the wallet was
              created speeds up syncing.
            </Typography>
            <SeedPhraseInput
              value={customSeed}
              onChange={setCustomSeed}
              error={!isSeedValid && customSeed.length > 0}
              helperText={
                isSeedValid
                  ? "Seed is valid"
                  : customSeed.length > 0
                    ? "Seed is invalid"
                    : ""
              }
            />
            <TextField
              type="text"
              inputProps={{ inputmode: "numeric", pattern: "[0-9]*" }}
              label="Restore blockheight (optional)"
              value={blockheightInput}
              onChange={(e) => setBlockheightInput(e.target.value)}
              placeholder="Enter restore blockheight, leave empty to scan from the blockchain start"
              error={isBlockheightInvalid}
              helperText={
                isBlockheightInvalid
                  ? "Please enter a valid blockheight"
                  : hasBlockheightInput && isBlockheightValid
                    ? "Valid blockheight"
                    : ""
              }
            />
          </Box>
        )}

        {step === "storage" && (
          <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            <Typography variant="body2" color="text.secondary">
              Set a password to encrypt the wallet file stored on this device.
              Leave it empty to store the wallet unencrypted.
            </Typography>
            <NewPasswordInput
              password={password}
              setPassword={setPassword}
              isPasswordValid={isPasswordValid}
              setIsPasswordValid={setIsPasswordValid}
            />
          </Box>
        )}

        {step === "nameLocation" && (
          <NameLocationStep
            name={name}
            setName={setName}
            directory={directory}
            setDirectory={setDirectory}
          />
        )}

        {step === "backupSeed" && (
          <BackupSeedStep onConfirmedChange={setBackupConfirmed} />
        )}

        {step === "openFile" && (
          <OpenWalletStep
            walletPath={walletPath}
            setWalletPath={setWalletPath}
            recentWallets={recentWallets}
          />
        )}
      </DialogContent>
      <DialogActions sx={{ justifyContent: "space-between" }}>
        <Box>
          {step === "chooseMode" && (
            <PromiseInvokeButton
              variant="text"
              onInvoke={() => resolve({ type: "Legacy" })}
              contextRequirement={false}
              color="inherit"
            >
              No wallet (Legacy)
            </PromiseInvokeButton>
          )}
          {index > 0 && wizard.phase === "editing" && (
            <Button
              variant="text"
              color="inherit"
              onClick={() => dispatch({ type: "back" })}
            >
              Back
            </Button>
          )}
        </Box>
        <PromiseInvokeButton
          variant="contained"
          disabled={!stepValid[step]}
          contextRequirement={false}
          onInvoke={async () => {
            await primaryHandlers[flow[index].action]();
          }}
        >
          {PRIMARY_LABELS[flow[index].action]}
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}

function ModeCard({
  selected,
  icon,
  label,
  onClick,
}: {
  selected: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <Card
      sx={{
        cursor: "pointer",
        border: selected ? 2 : 1,
        borderColor: selected ? "primary.main" : "divider",
        "&:hover": { borderColor: "primary.main" },
        flex: 1,
      }}
      onClick={onClick}
    >
      <CardContent
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 1,
        }}
      >
        {icon}
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ textAlign: "center" }}
        >
          {label}
        </Typography>
      </CardContent>
    </Card>
  );
}
