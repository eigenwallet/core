import { Button, Dialog, DialogActions, DialogContent } from "@mui/material";

import { DialogTitle } from "@mui/material";
import { useState } from "react";
import { setMoneroWalletPassword } from "renderer/rpc";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { isContextWithMoneroWallet } from "models/tauriModelExt";
import NewPasswordInput from "renderer/components/other/NewPasswordInput";

export default function ChangePasswordModal({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [password, setPassword] = useState<string>("");
  const [isPasswordValid, setIsPasswordValid] = useState<boolean>(true);

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Change Password</DialogTitle>
      <DialogContent sx={{ borderTop: "1em" }}>
        <NewPasswordInput
          password={password}
          setPassword={setPassword}
          isPasswordValid={isPasswordValid}
          setIsPasswordValid={setIsPasswordValid}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          disabled={!isPasswordValid}
          onInvoke={async () => await setMoneroWalletPassword(password)}
          onSuccess={onClose}
          displayErrorSnackbar={true}
          contextRequirement={isContextWithMoneroWallet}
        >
          Confirm
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
