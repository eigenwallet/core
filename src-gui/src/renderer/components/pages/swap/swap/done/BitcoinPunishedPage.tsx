import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Modal,
  TextField,
  Typography,
} from "@mui/material";
import FeedbackInfoBox from "renderer/components/pages/help/FeedbackInfoBox";
import { TauriSwapProgressEventExt } from "models/tauriModelExt";
import { useState } from "react";
import { resumeWithCooperativeRedeem, resumeSwap } from "renderer/rpc";
import { useActiveSwapId } from "store/hooks";
import { useSnackbar } from "notistack";

export default function BitcoinPunishedPage({
  state,
}: {
  state:
    | TauriSwapProgressEventExt<"BtcPunished">
    | TauriSwapProgressEventExt<"CooperativeRedeemRejected">;
}) {
  const [modalOpen, setModalOpen] = useState(
    state.type === "CooperativeRedeemRejected",
  );

  return (
    <>
      <DialogContentText>
        Unfortunately, the swap was unsuccessful. Since you did not refund in
        time, the Bitcoin has been lost. However, with the cooperation of the
        other party, you might still be able to redeem the Monero, although this
        is not guaranteed.{" "}
        {state.type === "CooperativeRedeemRejected" && (
          <>
            <br />
            We tried to redeem the Monero with the other party's help, but it
            was unsuccessful (reason: {state.content.reason}). Attempting again
            at a later time might yield success.
            <br />
          </>
        )}
      </DialogContentText>
      <FeedbackInfoBox />
      <ManualCoopRedeemModal
        open={modalOpen}
        onClose={() => setModalOpen(false)}
      />
    </>
  );
}

interface ManualCoopRedeemModalProps {
  open: boolean;
  onClose: () => void;
}

function ManualCoopRedeemModal({ open, onClose }: ManualCoopRedeemModalProps) {
  const [success, setSuccess] = useState<boolean | null>(null);
  const [inProgress, setInProgress] = useState(false);
  const [key, setKey] = useState("");
  const [txId, setTxId] = useState("");
  const [txKey, setTxKey] = useState("");
  const swapId = useActiveSwapId();

  const { enqueueSnackbar } = useSnackbar();

  const handleAttempt = async () => {
    setSuccess(null);
    setInProgress(true);

    // Try and use the given information to cooperatively redeem
    try {
      await resumeWithCooperativeRedeem(swapId, key, txId, txKey);
      onClose();
    } catch (e) {
      // If we get an error, throw it to the snackbar
      enqueueSnackbar<"error">(`Cooperative redeem failed: \`${e}\``);
      setSuccess(false);
    } finally {
      setInProgress(false);
      setKey("");
      setTxId("");
      setTxKey("");
    }
  };

  const alert = success ? (
    <Alert severity="success">
      Successfully verified key, attempting swap completion now.
    </Alert>
  ) : (
    <Alert severity="error">Couldn't verify the redeem key.</Alert>
  );

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Manual Cooperative Redeem</DialogTitle>
      <DialogContent>
        {success !== null ? alert : null}

        <Typography variant="caption">
          As a fallback to the automated cooperative redeem process, you can
          also ask your maker to provide these values and paste them below.
        </Typography>

        <TextField
          label={"Cooperative Redeem Key (s_a)"}
          onChange={(e) => setKey(e.target.value)}
        />
        <TextField
          label={"Monero Lock Transaction ID"}
          onChange={(e) => setTxId(e.target.value)}
        />
        <TextField
          label={"Monero Lock Transaction Key"}
          onChange={(e) => setTxKey(e.target.value)}
        />
      </DialogContent>
      <DialogActions>
        <Box sx={{ display: "flex", justifyContent: "space-between" }}>
          <Button variant="outlined" onClick={onClose}>
            Close
          </Button>
          <Button
            variant="contained"
            color="primary"
            onClick={handleAttempt}
            disabled={inProgress}
          >
            Attempt cooperative redeem
          </Button>
        </Box>
      </DialogActions>
    </Dialog>
  );
}
