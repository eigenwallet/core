import {
  Dialog,
  DialogContent,
  DialogTitle,
  Typography,
  ToggleButton,
  ToggleButtonGroup,
  Box,
  Link,
  Button,
  Tooltip,
} from "@mui/material";
import {
  DonateToDevelopmentTip,
  DONATE_TO_DEVELOPMENT_OPTIONS,
  setDonateToDevelopment,
} from "store/features/settingsSlice";
import {
  useAppDispatch,
  useSettings,
  useIsExperiencedUser,
  useIsIdle,
} from "store/hooks";
import ExternalLink from "renderer/components/other/ExternalLink";
import GitHubIcon from "@mui/icons-material/GitHub";
import { useState } from "react";

const GITHUB_BOUNTIES_URL = "https://eigenwallet.org/bounties";

// How long the user must be idle before showing the donation dialog
const DONATION_DIALOG_IDLE_DELAY_MS = 10 * 1000;

/** Formats a donation tip value as a percentage string (e.g., "0.5%", "1.2%", "0%") */
export function formatDonationTipLabel(tip: DonateToDevelopmentTip): string {
  if (tip === false || tip === 0) return "0%";
  return `${(tip * 100).toFixed(1)}%`;
}

// Helper functions for tip button styling (defined outside component to avoid recreation)
function getTipButtonColor(
  tip: Exclude<DonateToDevelopmentTip, false>,
): string {
  if (tip !== 0) {
    return "#198754"; // Green for any tip > 0
  }
  return "#6c757d"; // Gray for no tip
}

function getTipButtonHoverColor(
  tip: Exclude<DonateToDevelopmentTip, false>,
): string {
  if (tip === 0) return "#5c636a"; // Darker gray
  return "#146c43"; // Darker green
}

interface DonationTipDialogProps {
  open: boolean;
  onClose: () => void;
}

/**
 * Dialog for selecting a donation tip amount.
 */
export default function DonationTipDialog({
  open,
  onClose,
}: DonationTipDialogProps) {
  const dispatch = useAppDispatch();
  const [selectedTip, setSelectedTip] = useState<Exclude<
    DonateToDevelopmentTip,
    false
  > | null>(null);

  const handleConfirm = () => {
    if (selectedTip !== null) {
      dispatch(setDonateToDevelopment(selectedTip));
      onClose();
    }
  };

  return (
    <Dialog open={open} maxWidth="sm" fullWidth>
      <DialogTitle>Support the project?</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <Typography variant="subtitle2">
            Eigenwallet is an open source, community-driven project. It is
            funded entirely by the community.
          </Typography>
          <Typography variant="subtitle2">
            We use{" "}
            <ExternalLink href={GITHUB_BOUNTIES_URL}>
              <GitHubIcon
                sx={{ fontSize: "1em", verticalAlign: "middle", mr: 0.5 }}
              />
              <strong>Github bounties</strong>
            </ExternalLink>{" "}
            to fuel development efforts. Anyone can contribute to the project,
            fix an existing issue and claim the bounty.
          </Typography>
          <Typography variant="subtitle2">
            Bounties are funded with:
            <ol style={{ margin: 0, padding: 0, paddingLeft: "1.5rem" }}>
              <li>Funds left over from a previous Monero CCS</li>
              <li>
                Personal financial contributions by the core developer team
              </li>
              <li>Tips donated by users like you</li>
            </ol>
          </Typography>
          <Typography variant="subtitle2">
            If you enable the auto tip feature:
            <ul style={{ margin: 0, padding: 0, paddingLeft: "1.5rem" }}>
              <li>
                A <strong>small tip</strong> will be donated for each successful
                swap
              </li>
              <li>
                Tips will go <strong>directly</strong> to open source
                contributors
              </li>
              <li>Monero is used for the tips, giving you full privacy</li>
              <li>You can disable this at any time in the settings</li>
            </ul>
          </Typography>
          <ToggleButtonGroup
            value={selectedTip}
            exclusive
            onChange={(_, newValue) => {
              if (newValue !== null) {
                setSelectedTip(newValue);
              }
            }}
            aria-label="Development tip amount"
            size="small"
            sx={{
              width: "100%",
              gap: 1,
              "& .MuiToggleButton-root": {
                flex: 1,
                borderRadius: "8px",
                fontWeight: "600",
                textTransform: "none",
                border: "2px solid",
                "&:not(:first-of-type)": {
                  marginLeft: "8px",
                  borderLeft: "2px solid",
                },
              },
            }}
          >
            {DONATE_TO_DEVELOPMENT_OPTIONS.map((tipAmount) => (
              <ToggleButton
                key={String(tipAmount)}
                value={tipAmount}
                sx={{
                  borderColor: `${getTipButtonColor(tipAmount)} !important`,
                  color:
                    selectedTip === tipAmount
                      ? "white"
                      : getTipButtonColor(tipAmount),
                  backgroundColor:
                    selectedTip === tipAmount
                      ? getTipButtonColor(tipAmount)
                      : "transparent",
                  "&:hover": {
                    backgroundColor: `${getTipButtonHoverColor(tipAmount)} !important`,
                    color: "white !important",
                  },
                  "&.Mui-selected": {
                    backgroundColor: `${getTipButtonColor(tipAmount)} !important`,
                    color: "white !important",
                  },
                }}
              >
                {formatDonationTipLabel(tipAmount)}
              </ToggleButton>
            ))}
          </ToggleButtonGroup>
          <Box
            sx={{
              display: "flex",
              justifyContent: "flex-end",
              alignItems: "center",
              gap: 2,
            }}
          >
            <Link
              component="button"
              variant="body2"
              onClick={onClose}
              sx={{
                color: "text.secondary",
                opacity: selectedTip !== null ? 0.5 : 1,
              }}
            >
              Remind me later
            </Link>
            <Tooltip
              title={selectedTip === null ? "Select an option first" : ""}
              arrow
            >
              <span>
                <Button
                  variant="contained"
                  onClick={handleConfirm}
                  disabled={selectedTip === null}
                >
                  Done
                </Button>
              </span>
            </Tooltip>
          </Box>
        </Box>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Wrapper that shows DonationTipDialog automatically based on global state.
 * Shows when user hasn't selected a tip yet, is an experienced user, and has been idle.
 */
export function GlobalDonationTipDialog() {
  // Has the user selected the tip any option yet?
  const hasntSelectedTipYet = useSettings(
    (s) => s.donateToDevelopment === false,
  );

  // Has the user used the app at least a little bit?
  const isExperiencedUser = useIsExperiencedUser();

  // Has the user been idle?
  const isIdle = useIsIdle(DONATION_DIALOG_IDLE_DELAY_MS);

  // Track if user dismissed the dialog this session (don't show again until next app launch)
  const [dismissed, setDismissed] = useState(false);

  // "Latch" pattern: once conditions are met, we remember that the dialog should be shown.
  // This prevents the dialog from closing when the user moves their mouse (which would
  // break the idle condition). Once triggered, it stays triggered.
  // Uses "adjust state during render" pattern instead of useEffect.
  const [hasTriggered, setHasTriggered] = useState(false);

  const shouldOpen = hasntSelectedTipYet && isExperiencedUser && isIdle;

  if (shouldOpen && !hasTriggered) {
    setHasTriggered(true);
  }

  // Show dialog if we've triggered and user hasn't dismissed
  const open = hasTriggered && !dismissed;

  return <DonationTipDialog open={open} onClose={() => setDismissed(true)} />;
}
