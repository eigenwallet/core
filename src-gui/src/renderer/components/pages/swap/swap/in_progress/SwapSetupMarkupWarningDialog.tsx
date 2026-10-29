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

type SwapSetupMarkupWarningDialogProps = {
  open: boolean;
  onClose: () => void;
  onAccept: () => Promise<unknown>;
  onDeny: () => Promise<unknown>;
  /// The rate of the selected quote in BTC per XMR. Null if the GUI did not
  /// receive a quote for this swap.
  quotedRate: number | null;
  /// The rate of the offer during swap setup in BTC per XMR.
  offeredRate: number;
  timeLeft: number;
};

export default function SwapSetupMarkupWarningDialog({
  open,
  onClose,
  onAccept,
  onDeny,
  quotedRate,
  offeredRate,
  timeLeft,
}: SwapSetupMarkupWarningDialogProps) {
  const [isPending, setIsPending] = useState(false);

  const handleClose = () => {
    if (isPending) {
      return;
    }

    onClose();
  };

  return (
    <Dialog
      open={open}
      onClose={handleClose}
      disableEscapeKeyDown={isPending}
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
            Are you sure this is the swap exchange that you want to accept?
          </Typography>
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
          <Typography variant="caption" color="text.secondary">
            {`Offer expires in ${timeLeft}s`}
          </Typography>
        </Box>
      </DialogContent>
      <DialogActions sx={{ justifyContent: "center", gap: 1, pb: 3 }}>
        <Button onClick={handleClose} disabled={isPending}>
          Back
        </Button>
        <PromiseInvokeButton
          variant="outlined"
          color="primary"
          onInvoke={onDeny}
          onPendingChange={setIsPending}
          disabled={isPending}
          displayErrorSnackbar
        >
          Deny
        </PromiseInvokeButton>
        <PromiseInvokeButton
          variant="contained"
          color="error"
          onInvoke={onAccept}
          onPendingChange={setIsPending}
          disabled={isPending || timeLeft <= 0}
          displayErrorSnackbar
        >
          Accept
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
