import {
  Button,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  DialogContentText,
  Link,
} from "@mui/material";
import { Key as KeyIcon } from "@mui/icons-material";
import { useState } from "react";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { getWalletDescriptor } from "renderer/rpc";
import { ExportBitcoinWalletResponse } from "models/tauriModel";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { isContextWithBitcoinWallet } from "models/tauriModelExt";

const WALLET_DESCRIPTOR_DOCS_URL =
  "https://github.com/eigenwallet/core/blob/master/dev-docs/asb/README.md#exporting-the-bitcoin-wallet-descriptor";

export default function WalletDescriptorButton() {
  const [walletDescriptor, setWalletDescriptor] =
    useState<ExportBitcoinWalletResponse | null>(null);

  const handleCloseDialog = () => {
    setWalletDescriptor(null);
  };

  return (
    <>
      <PromiseInvokeButton
        isChipButton={true}
        startIcon={<KeyIcon />}
        onInvoke={getWalletDescriptor}
        onSuccess={setWalletDescriptor}
        displayErrorSnackbar={true}
        contextRequirement={isContextWithBitcoinWallet}
      >
        Reveal Private Key
      </PromiseInvokeButton>
      {walletDescriptor !== null && (
        <WalletDescriptorModal
          open={walletDescriptor !== null}
          onClose={handleCloseDialog}
          walletDescriptor={walletDescriptor}
        />
      )}
    </>
  );
}

function WalletDescriptorModal({
  open,
  onClose,
  walletDescriptor,
}: {
  open: boolean;
  onClose: () => void;
  walletDescriptor: ExportBitcoinWalletResponse;
}) {
  const parsedDescriptor = JSON.parse(
    walletDescriptor.wallet_descriptor["descriptor"],
  );
  const stringifiedDescriptor = JSON.stringify(parsedDescriptor, null, 4);

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Bitcoin Wallet Descriptor</DialogTitle>
      <DialogContent>
        <DialogContentText>
          The Bitcoin wallet is derived from your Monero wallet. Opening the
          same Monero wallet in another Eigenwallet will yield the same Bitcoin
          wallet.
          <br />
          <br />
          It contains your private key. Anyone who has it can spend your funds.
          It should thus be stored securely.
          <br />
          <br />
          It can be imported into other Bitcoin wallets or services that support
          the descriptor format. For more information on what to do with the
          descriptor, see our{" "}
          <Link href={WALLET_DESCRIPTOR_DOCS_URL} target="_blank">
            documentation
          </Link>
        </DialogContentText>
        <ActionableMonospaceTextBox
          content={stringifiedDescriptor}
          displayCopyIcon={true}
          enableQrCode={false}
          spoilerText="Press to Reveal Private Key"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="primary" variant="contained">
          Done
        </Button>
      </DialogActions>
    </Dialog>
  );
}
