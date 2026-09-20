import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Link,
} from "@mui/material";
import { ButtonProps } from "@mui/material/Button";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import ActionableMonospaceTextBox from "./ActionableMonospaceTextBox";
import { captionLinkSx } from "./captionLinkSx";

const XMRCHAIN_RAW_TX_URL = "https://xmrchain.net/rawtx";
const FEATHER_PUSH_TX_URL = "https://docs.featherwallet.org/guides/push-tx";

export default function MoneroRawTransactionButton({
  txHex,
  children = "Show raw transaction",
  ...props
}: { txHex: string } & Omit<ButtonProps, "onClick">) {
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <>
      <Button
        variant="text"
        sx={captionLinkSx}
        onClick={() => setDialogOpen(true)}
        {...props}
      >
        {children}
      </Button>

      <Dialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>Monero redeem transaction</DialogTitle>
        <DialogContent>
          <DialogContentText>
            You can publish this transaction yourself by pasting the hex below
            into{" "}
            <Link
              style={{ cursor: "pointer" }}
              onClick={() => openUrl(XMRCHAIN_RAW_TX_URL)}
            >
              {XMRCHAIN_RAW_TX_URL}
            </Link>
            , or broadcast it from{" "}
            <Link
              style={{ cursor: "pointer" }}
              onClick={() => openUrl(FEATHER_PUSH_TX_URL)}
            >
              Feather wallet
            </Link>
            .
          </DialogContentText>
          <Box sx={{ mt: 2 }}>
            <ActionableMonospaceTextBox content={txHex} enableQrCode={false} />
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => openUrl(XMRCHAIN_RAW_TX_URL)} color="primary">
            Open xmrchain.net
          </Button>
          <Button onClick={() => openUrl(FEATHER_PUSH_TX_URL)} color="primary">
            Open Feather guide
          </Button>
          <Button onClick={() => setDialogOpen(false)}>Close</Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
