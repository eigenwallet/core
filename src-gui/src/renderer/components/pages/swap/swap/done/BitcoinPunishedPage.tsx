import {
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
import { manualCooperativeRedeem, resumeSwap } from "renderer/rpc";
import { useActiveSwapId } from "store/hooks";

export default function BitcoinPunishedPage({
  state,
}: {
  state:
    | TauriSwapProgressEventExt<"BtcPunished">
    | TauriSwapProgressEventExt<"CooperativeRedeemRejected">;
}) {
  const [modalOpen, setModalOpen] = useState(false);

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
        onClose={() => setModalOpen(true)}
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
  const [key, setKey] = useState("");
  const [txId, setTxId] = useState("");
  const [txKey, setTxKey] = useState("");
  const swapId = useActiveSwapId();

  const handleAttempt = async () => {
    setSuccess(null);
    try {
      await manualCooperativeRedeem(swapId, key, txId, txKey);
      setSuccess(true);
    } catch (e) {
      console.error("Failed to cooperatively redeem: " + e);
      setSuccess(false);
    } finally {
      setKey("");
      setTxId("");
      setTxKey("");
    }

    // Wait 5 seconds to give user time to read message
    await new Promise((res) => setTimeout(res, 5000));

    // Close the modal and continue the swap normally if the cooperative redeem succeded
    if (success) {
      onClose();
      resumeSwap(swapId);
    }
  };

  const resultText = success
    ? "Success, resuming swap"
    : success === false
      ? "Oops, failed (see console for error)"
      : "";

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Manual Cooperative Redeem</DialogTitle>
      <DialogContent>
        {resultText}

        <Typography variant="caption">
          As a fallback to the automated cooperative redeem process, you can
          also ask your maker to provide these values and paste them below.
        </Typography>

        <TextField
          label={"Secret Redeem Key"}
          onChange={(e) => setKey(e.target.value)}
        />
        <TextField
          label={"Lock Transaction ID"}
          onChange={(e) => setTxId(e.target.value)}
        />
        <TextField
          label={"Lock Transaction Key"}
          onChange={(e) => setTxKey(e.target.value)}
        />
      </DialogContent>
      <DialogActions>
        <Box sx={{ display: "flex", justifyContent: "space-between" }}>
          <Button variant="outlined" onClick={onClose}>
            Close
          </Button>
          <Button variant="contained" color="primary" onClick={handleAttempt}>
            Attempt
          </Button>
        </Box>
      </DialogActions>
    </Dialog>
  );
}
