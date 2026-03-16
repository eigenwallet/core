import { Alert } from "@mui/material";


/** An alert warning users not to reveal their private key to anyone. */
export function PrivateKeyScamAlert() {
  return <Alert severity="warning">
    <b>Warning!</b> This is your private key.
    It gives immediate access to all funds in the wallet.
    <br />
    <b>Never</b> share it with anyone. <b>Even the devs will never ask for your private key.</b>
    <br />
    Anyone who asks for your private key is trying to <b>scam</b> you.
  </Alert>;
}

