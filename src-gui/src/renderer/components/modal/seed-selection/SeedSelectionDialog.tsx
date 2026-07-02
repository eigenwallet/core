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
import {
  usePendingSeedSelectionApproval,
  usePendingSeedBackupApproval,
} from "store/hooks";
import { resolveApproval, checkSeed } from "renderer/rpc";
import { SeedChoice } from "models/tauriModel";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import AddIcon from "@mui/icons-material/Add";
import RefreshIcon from "@mui/icons-material/Refresh";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import BackupSeedStep from "./BackupSeedStep";
import OpenWalletStep from "./OpenWalletStep";
import NameLocationStep from "./NameLocationStep";
import CircularProgressWithSubtitle from "renderer/components/pages/swap/swap/components/CircularProgressWithSubtitle";

// The wallet-setup flow itself is a state machine in Rust
// (swap/src/cli/api/wallet_setup.rs): the backend walks
// ChooseSeed -> OpenWallet -> BackupSeed -> Finish and blocks on an approval
// whenever it needs user input. This dialog only renders whichever request is
// pending — a SeedSelection approval shows the wizard below, a SeedBackup
// approval shows the backup step — plus a local spinner while the backend is
// busy creating the wallet between the two.
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

/// Everything the user types into the wizard.
interface FormFields {
  password: string;
  customSeed: string;
  blockheightInput: string;
  walletPath: string;
  name: string;
  directory: string;
}

const EMPTY_FIELDS: FormFields = {
  password: "",
  customSeed: "",
  blockheightInput: "",
  walletPath: "",
  name: "",
  directory: "",
};

// Wizard navigation state for filling out a SeedSelection approval. This is
// purely presentational: mode/step navigate the FLOWS table, fields hold the
// user's input, and a reset rebuilds everything for a new request.
interface WizardState {
  mode: WalletMode;
  step: StepId;
  fields: FormFields;
}

type WizardEvent =
  | { type: "reset"; mode: WalletMode; fields: Partial<FormFields> }
  | { type: "selectMode"; mode: WalletMode }
  | { type: "setField"; field: keyof FormFields; value: string }
  | { type: "next" }
  | { type: "back" }
  | { type: "navigateBackTo"; step: StepId };

const INITIAL_WIZARD: WizardState = {
  mode: "RandomSeed",
  step: "chooseMode",
  fields: EMPTY_FIELDS,
};

// Every transition is guarded: an event that does not apply to the current
// state is a no-op instead of producing an invalid state.
function wizardReducer(state: WizardState, event: WizardEvent): WizardState {
  const flow = FLOWS[state.mode];
  const index = stepIndex(state.mode, state.step);

  switch (event.type) {
    case "reset":
      return {
        mode: event.mode,
        step: "chooseMode",
        fields: { ...EMPTY_FIELDS, ...event.fields },
      };
    case "setField":
      return {
        ...state,
        fields: { ...state.fields, [event.field]: event.value },
      };
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
  const selectionApproval = usePendingSeedSelectionApproval()[0];
  const backupApproval = usePendingSeedBackupApproval()[0];
  const [wizard, dispatch] = useReducer(wizardReducer, INITIAL_WIZARD);
  const [isPasswordValid, setIsPasswordValid] = useState<boolean>(true);
  const [backupConfirmed, setBackupConfirmed] = useState<boolean>(false);
  // Backend is creating the wallet: the SeedSelection approval is resolved
  // but the SeedBackup approval has not arrived yet.
  const [waitingForWallet, setWaitingForWallet] = useState<boolean>(false);
  // Result of the async seed check, tagged with the seed it belongs to so a
  // stale response can never validate a newer input.
  const [seedValidation, setSeedValidation] = useState<{
    forSeed: string;
    valid: boolean;
  } | null>(null);

  const content =
    selectionApproval?.request?.type === "SeedSelection"
      ? selectionApproval.request.content
      : undefined;
  const recentWallets = content?.recent_wallets ?? [];
  const backupContent =
    backupApproval?.request?.type === "SeedBackup"
      ? backupApproval.request.content
      : undefined;

  // Reset the wizard whenever a new seed-selection approval arrives (e.g.
  // after the user cancels a password prompt or a wallet fails to create and
  // the backend asks again).
  const lastRequestIdRef = useRef<string | null>(null);
  useEffect(() => {
    const requestId = selectionApproval?.request_id;
    if (!requestId || requestId === lastRequestIdRef.current) return;
    lastRequestIdRef.current = requestId;

    // Default to opening a recent wallet when one exists, otherwise create.
    dispatch({
      type: "reset",
      mode: recentWallets.length > 0 ? "FromWalletPath" : "RandomSeed",
      fields: {
        directory: content?.default_wallet_directory ?? "",
        walletPath: recentWallets[0] ?? "",
      },
    });
    setSeedValidation(null);
    setWaitingForWallet(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset only on a new approval id
  }, [selectionApproval?.request_id]);

  // A backup request means the wallet was created: ask for a fresh
  // confirmation.
  useEffect(() => {
    if (!backupApproval) return;
    setWaitingForWallet(false);
    setBackupConfirmed(false);
  }, [backupApproval?.request_id, backupApproval]);

  const trimmedSeed = wizard.fields.customSeed.trim();
  const needsSeedValidation =
    wizard.mode === "FromSeed" && trimmedSeed.length > 0;

  useEffect(() => {
    if (!needsSeedValidation) return;

    checkSeed(trimmedSeed)
      .then((valid) => setSeedValidation({ forSeed: trimmedSeed, valid }))
      .catch(() => setSeedValidation({ forSeed: trimmedSeed, valid: false }));
  }, [trimmedSeed, needsSeedValidation]);

  // Which screen is shown is decided by the backend's pending request.
  const view = backupApproval
    ? ("backup" as const)
    : selectionApproval
      ? ("wizard" as const)
      : waitingForWallet
        ? ("waiting" as const)
        : null;

  if (view === null) return null;

  // The backup and waiting screens render the last step of the create flow.
  const mode = view === "wizard" ? wizard.mode : "RandomSeed";
  const step: StepId = view === "wizard" ? wizard.step : "backupSeed";
  const flow = FLOWS[mode];
  const index = stepIndex(mode, step);
  const { fields } = wizard;

  const setField = (field: keyof FormFields) => (value: string) =>
    dispatch({ type: "setField", field, value });

  const isSeedValid =
    needsSeedValidation &&
    seedValidation !== null &&
    seedValidation.forSeed === trimmedSeed &&
    seedValidation.valid;
  const hasBlockheightInput = fields.blockheightInput.length > 0;
  const isBlockheightValid =
    parseBlockHeightInput(fields.blockheightInput) !== false;
  const isBlockheightInvalid = hasBlockheightInput && !isBlockheightValid;

  // Whether the current step's input is complete enough to act on.
  const stepValid: Record<StepId, boolean> = {
    chooseMode: true,
    randomPassword: isPasswordValid,
    seedPhrase: trimmedSeed.length > 0 && isSeedValid && !isBlockheightInvalid,
    storage: isPasswordValid,
    nameLocation:
      fields.name.trim().length > 0 && fields.directory.trim().length > 0,
    openFile: fields.walletPath.length > 0,
    backupSeed: view === "backup" && backupConfirmed,
  };

  const buildSeedChoice = (chosenMode: WalletMode): SeedChoice => {
    switch (chosenMode) {
      case "RandomSeed":
        return {
          type: "RandomSeed",
          content: {
            password: fields.password,
            name: fields.name,
            directory: fields.directory,
          },
        };
      case "FromSeed": {
        const parsedBlockHeight = parseBlockHeightInput(
          fields.blockheightInput,
        );
        if (parsedBlockHeight === false) {
          throw new Error("Invalid blockheight");
        }
        return {
          type: "FromSeed",
          content: {
            seed: fields.customSeed,
            password: fields.password,
            restore_height: parsedBlockHeight,
            name: fields.name,
            directory: fields.directory,
          },
        };
      }
      case "FromWalletPath":
        return {
          type: "FromWalletPath",
          content: { wallet_path: fields.walletPath },
        };
    }
  };

  const resolveSelection = async (seedChoice: SeedChoice) => {
    if (!selectionApproval)
      throw new Error("No approval request found for seed selection");
    await resolveApproval<SeedChoice>(selectionApproval.request_id, seedChoice);
  };

  // The backend creates the wallet after the choice is resolved and then
  // requests the seed backup; show a spinner in between.
  const createWallet = async () => {
    const seedChoice = buildSeedChoice("RandomSeed");
    setWaitingForWallet(true);
    try {
      await resolveSelection(seedChoice);
    } catch (e) {
      setWaitingForWallet(false);
      throw e;
    }
  };

  const finishBackup = async () => {
    if (!backupApproval)
      throw new Error("No approval request found for seed backup");
    await resolveApproval<boolean>(backupApproval.request_id, true);
  };

  const primaryHandlers: Record<PrimaryActionKind, () => void | Promise<void>> =
    {
      next: () => dispatch({ type: "next" }),
      finish: finishBackup,
      createWallet,
      restoreWallet: () => resolveSelection(buildSeedChoice("FromSeed")),
      openWallet: () => resolveSelection(buildSeedChoice("FromWalletPath")),
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
            const navigable = i < index && view === "wizard";
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
              password={fields.password}
              setPassword={setField("password")}
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
            <TextField
              fullWidth
              multiline
              autoFocus
              rows={3}
              label="Enter your seed phrase"
              value={fields.customSeed}
              onChange={(e) => setField("customSeed")(e.target.value)}
              placeholder="Enter your Monero 25 words seed phrase..."
              error={!isSeedValid && fields.customSeed.length > 0}
              helperText={
                isSeedValid
                  ? "Seed is valid"
                  : fields.customSeed.length > 0
                    ? "Seed is invalid"
                    : ""
              }
            />
            <TextField
              type="text"
              inputProps={{ inputmode: "numeric", pattern: "[0-9]*" }}
              label="Restore blockheight (optional)"
              value={fields.blockheightInput}
              onChange={(e) => setField("blockheightInput")(e.target.value)}
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
              password={fields.password}
              setPassword={setField("password")}
              isPasswordValid={isPasswordValid}
              setIsPasswordValid={setIsPasswordValid}
            />
          </Box>
        )}

        {step === "nameLocation" && (
          <NameLocationStep
            name={fields.name}
            setName={setField("name")}
            directory={fields.directory}
            setDirectory={setField("directory")}
          />
        )}

        {step === "backupSeed" &&
          (backupContent ? (
            <BackupSeedStep
              seed={backupContent.seed}
              restoreHeight={backupContent.restore_height}
              confirmed={backupConfirmed}
              onConfirmedChange={setBackupConfirmed}
            />
          ) : (
            <CircularProgressWithSubtitle description="Creating your new wallet…" />
          ))}

        {step === "openFile" && (
          <OpenWalletStep
            walletPath={fields.walletPath}
            setWalletPath={setField("walletPath")}
            recentWallets={recentWallets}
          />
        )}
      </DialogContent>
      <DialogActions sx={{ justifyContent: "space-between" }}>
        <Box>
          {step === "chooseMode" && (
            <PromiseInvokeButton
              variant="text"
              onInvoke={() => resolveSelection({ type: "Legacy" })}
              contextRequirement={false}
              displayErrorSnackbar
              color="inherit"
            >
              No wallet (Legacy)
            </PromiseInvokeButton>
          )}
          {index > 0 && view === "wizard" && (
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
          displayErrorSnackbar
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
