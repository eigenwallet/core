import { Button, DialogActions } from "@mui/material";
import MobileDialog from "../MobileDialog";
import MobileDialogHeader from "../MobileDialogHeader";
import { useState } from "react";
import PromiseInvokeButton from "renderer/components/buttons/PromiseInvokeButton";
import { withdrawBtc } from "renderer/rpc";
import DialogHeader from "../DialogHeader";
import AddressInputPage from "./pages/AddressInputPage";
import BtcTxInMempoolPageContent from "./pages/BitcoinWithdrawTxInMempoolPage";
import WithdrawDialogContent from "./WithdrawDialogContent";

export default function WithdrawDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [pending, setPending] = useState(false);
  const [withdrawTxId, setWithdrawTxId] = useState<string | null>(null);
  const [withdrawAddressValid, setWithdrawAddressValid] = useState(false);
  const [withdrawAddress, setWithdrawAddress] = useState<string>("");

  const haveFundsBeenWithdrawn = withdrawTxId !== null;

  function onCancel() {
    if (!pending) {
      setWithdrawTxId(null);
      setWithdrawAddress("");
      onClose();
    }
  }

  return (
    <MobileDialog open={open} onClose={onCancel} maxWidth="sm" fullWidth>
      <MobileDialogHeader title="Withdraw Bitcoin" onClose={onCancel} />
      <DialogHeader title="Withdraw Bitcoin" />
      <WithdrawDialogContent isPending={pending} withdrawTxId={withdrawTxId}>
        {haveFundsBeenWithdrawn ? (
          <BtcTxInMempoolPageContent withdrawTxId={withdrawTxId} />
        ) : (
          <AddressInputPage
            setWithdrawAddress={setWithdrawAddress}
            withdrawAddress={withdrawAddress}
            setWithdrawAddressValid={setWithdrawAddressValid}
          />
        )}
      </WithdrawDialogContent>
      <DialogActions>
        <Button
          onClick={onCancel}
          color="primary"
          disabled={pending}
          variant={haveFundsBeenWithdrawn ? "contained" : "text"}
        >
          {haveFundsBeenWithdrawn ? "Done" : "Close"}
        </Button>
        {!haveFundsBeenWithdrawn && (
          <PromiseInvokeButton
            displayErrorSnackbar
            variant="contained"
            color="primary"
            disabled={!withdrawAddressValid}
            onInvoke={() => withdrawBtc(withdrawAddress)}
            onPendingChange={setPending}
            onSuccess={setWithdrawTxId}
          >
            Withdraw
          </PromiseInvokeButton>
        )}
      </DialogActions>
    </MobileDialog>
  );
}
