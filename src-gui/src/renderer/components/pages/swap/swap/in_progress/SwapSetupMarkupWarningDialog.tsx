import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  Typography,
} from "@mui/material";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useState } from "react";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { MoneroBitcoinExchangeRate } from "renderer/components/other/Units";
import { SwapSetupRateWarning } from "utils/swapSetupMarkup";

type SwapSetupMarkupWarningDialogProps = {
  open: boolean;
  warning: SwapSetupRateWarning;
  /// Closes the dialog. The user then confirms the offer on the page behind it.
  onContinue: () => void;
  onDeny: () => Promise<unknown>;
  /// The rate of the selected quote in BTC per XMR. Null if the GUI did not
  /// receive a quote for this swap.
  quotedRate: number | null;
  /// The rate of the offer during swap setup in BTC per XMR.
  offeredRate: number;
  timeLeft: number;
};

function getWarningText(warning: SwapSetupRateWarning): string {
  switch (warning.type) {
    case "MarkupHigh":
      return `This offer has a markup of ${warning.offeredMarkupPercent.toFixed(2)}%, which is quite high.`;
    case "MarkupHighAndRaised":
      return `This offer has a markup of ${warning.offeredMarkupPercent.toFixed(2)}%, which is quite high and higher than quoted.`;
    case "MarkupRaised":
      return "The markup of this offer is higher than quoted.";
    case "RateRaised":
      return "The rate of this offer is higher than quoted.";
    case "NoQuote":
      return "There is no quote to compare this offer with.";
  }
}

export default function SwapSetupMarkupWarningDialog({
  open,
  warning,
  onContinue,
  onDeny,
  quotedRate,
  offeredRate,
  timeLeft,
}: SwapSetupMarkupWarningDialogProps) {
  const [isDenying, setIsDenying] = useState(false);

  return (
    <Dialog
      open={open}
      aria-labelledby="swap-setup-markup-warning-title"
      maxWidth="sm"
      fullWidth
    >
      <DialogContent>
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            textAlign: "center",
            gap: 2,
            pt: 1,
          }}
        >
          <WarningAmberIcon color="error" sx={{ fontSize: "4rem" }} />
          <Typography
            id="swap-setup-markup-warning-title"
            variant="h5"
            sx={{ fontWeight: "bold" }}
          >
            Are you sure this is the exchange rate that you want to accept?
          </Typography>
          <Typography variant="body1">{getWarningText(warning)}</Typography>
          {warning.type !== "MarkupHigh" && (
            <Box
              sx={{
                display: "grid",
                gridTemplateColumns: "auto auto",
                columnGap: 2,
                rowGap: 0.5,
                textAlign: "left",
              }}
            >
              <Typography variant="body2" color="text.secondary">
                Quoted rate
              </Typography>
              <Typography variant="body2">
                {quotedRate == null ? (
                  "Unknown"
                ) : (
                  <MoneroBitcoinExchangeRate rate={quotedRate} displayMarkup />
                )}
              </Typography>
              <Typography variant="body2" color="text.secondary">
                Offered rate
              </Typography>
              <Typography
                variant="body2"
                sx={{ fontWeight: "bold", color: "error.main" }}
              >
                <MoneroBitcoinExchangeRate rate={offeredRate} displayMarkup />
              </Typography>
            </Box>
          )}
          <Typography variant="caption" color="text.secondary">
            {`Offer expires in ${timeLeft}s`}
          </Typography>
        </Box>
      </DialogContent>
      <DialogActions sx={{ justifyContent: "center", gap: 1, pb: 3 }}>
        <PromiseInvokeButton
          variant="outlined"
          color="primary"
          onInvoke={onDeny}
          onPendingChange={setIsDenying}
          displayErrorSnackbar
        >
          Deny
        </PromiseInvokeButton>
        <Button
          variant="contained"
          color="error"
          onClick={onContinue}
          disabled={isDenying || timeLeft <= 0}
        >
          Continue
        </Button>
      </DialogActions>
    </Dialog>
  );
}
