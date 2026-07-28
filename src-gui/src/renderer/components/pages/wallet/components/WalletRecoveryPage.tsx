import { ArrowBack as ArrowBackIcon } from "@mui/icons-material";
import { Box, Button, Card, Typography } from "@mui/material";
import { hasDescriptorProperty } from "models/tauriModelExt";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { WalletRecoveryData } from "renderer/rpc";

function DerivationNode({
  title,
  detail,
  active = false,
}: {
  title: string;
  detail: string;
  active?: boolean;
}) {
  return (
    <Box
      sx={{
        border: 1,
        borderColor: active ? "primary.main" : "divider",
        borderRadius: 1,
        p: 1.5,
        textAlign: "center",
        bgcolor: active ? "action.selected" : "background.paper",
      }}
    >
      <Typography variant="subtitle2">{title}</Typography>
      <Typography variant="caption" color="text.secondary">
        {detail}
      </Typography>
    </Box>
  );
}

function DerivationArrow() {
  return (
    <Typography
      aria-hidden="true"
      color="text.secondary"
      sx={{ lineHeight: 1.5, textAlign: "center" }}
    >
      ↓
    </Typography>
  );
}

function SharedSecretDerivation({
  highlightedWallet,
}: {
  highlightedWallet: "bitcoin" | "monero";
}) {
  const moneroHighlighted = highlightedWallet === "monero";
  const bitcoinHighlighted = highlightedWallet === "bitcoin";

  return (
    <Box aria-label="Wallet seed derivation hierarchy" sx={{ my: 1 }}>
      <DerivationNode
        title="Shared 32-byte secret"
        detail="The same 256 bits secure both wallets"
      />
      <Box
        aria-hidden="true"
        sx={{
          display: { xs: "none", sm: "block" },
          height: 20,
          width: "50%",
          mx: "auto",
          borderTop: 1,
          borderLeft: 1,
          borderRight: 1,
          borderColor: "divider",
        }}
      />
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: { xs: "1fr", sm: "1fr 1fr" },
          gap: 2,
        }}
      >
        <Box>
          <Box sx={{ display: { xs: "block", sm: "none" } }}>
            <DerivationArrow />
          </Box>
          <DerivationNode
            title="Monero seed phrase"
            detail="The shared secret encoded as 25 words"
            active={moneroHighlighted}
          />
          <DerivationArrow />
          <DerivationNode
            title="Monero wallet"
            detail="Restore with the Monero phrase"
            active={moneroHighlighted}
          />
        </Box>
        <Box>
          <Box sx={{ display: { xs: "block", sm: "none" } }}>
            <DerivationArrow />
          </Box>
          <DerivationNode
            title="Bitcoin seed phrase"
            detail="The shared secret encoded as 24 BIP39 words"
            active={bitcoinHighlighted}
          />
          <DerivationArrow />
          <DerivationNode
            title="Bitcoin wallet"
            detail="Restore with the Bitcoin phrase"
            active={bitcoinHighlighted}
          />
        </Box>
      </Box>
    </Box>
  );
}

export default function WalletRecoveryPage({
  walletRecovery,
  highlightedWallet,
  onBack,
}: {
  walletRecovery: WalletRecoveryData;
  highlightedWallet: "bitcoin" | "monero";
  onBack: () => void;
}) {
  const { walletDescriptor, moneroSeed, restoreHeight } = walletRecovery;

  if (!hasDescriptorProperty(walletDescriptor)) {
    throw new Error("Wallet descriptor does not have descriptor property");
  }

  const {
    seed_phrase: seedPhrase,
    legacy_descriptor: legacyDescriptor,
    legacy_change_descriptor: legacyChangeDescriptor,
  } = walletDescriptor.wallet_descriptor;

  return (
    <>
      <Box
        sx={{
          position: "sticky",
          top: 0,
          zIndex: 1,
          display: "flex",
          alignItems: "center",
          gap: 1,
          py: 1,
          bgcolor: "background.default",
        }}
      >
        <Button startIcon={<ArrowBackIcon />} onClick={onBack}>
          Back
        </Button>
        <Typography variant="h5">Wallet Recovery</Typography>
      </Box>
      <Card
        elevation={4}
        sx={{ display: "flex", flexDirection: "column", gap: 2, p: 2 }}
      >
        <SharedSecretDerivation highlightedWallet={highlightedWallet} />
        <ActionableMonospaceTextBox
          title="Monero seed phrase"
          content={moneroSeed.seed}
          displayCopyIcon={true}
          enableQrCode={false}
          spoilerText="Press to Reveal Seed Phrase"
        />
        <ActionableMonospaceTextBox
          title="Monero restore height"
          content={restoreHeight.height.toString()}
          displayCopyIcon={true}
          enableQrCode={false}
        />
        <ActionableMonospaceTextBox
          title="Bitcoin seed phrase"
          content={seedPhrase}
          displayCopyIcon={true}
          enableQrCode={false}
          spoilerText="Press to Reveal Seed Phrase"
        />
        <ActionableMonospaceTextBox
          title="Legacy external descriptor"
          content={legacyDescriptor}
          displayCopyIcon={true}
          enableQrCode={false}
          spoilerText="Press to Reveal Legacy Descriptor"
        />
        <ActionableMonospaceTextBox
          title="Legacy change descriptor"
          content={legacyChangeDescriptor}
          displayCopyIcon={true}
          enableQrCode={false}
          spoilerText="Press to Reveal Legacy Change Descriptor"
        />
      </Card>
    </>
  );
}
