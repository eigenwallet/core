# GPG Signature Verification

This directory contains public GPG keys for verifying release binary signatures.

## Verifying Release Binaries

Each release includes `.asc` signature files alongside the binaries.

### 1. Import the signing key

```bash
# Download the key from GitHub
wget https://raw.githubusercontent.com/eigenwallet/core/master/utils/gpg_keys/binarybaron_and_einliterflasche.asc

# Import it
gpg --import binarybaron_and_einliterflasche.asc
```

### 2. Download and verify the signature

```bash
# Download both the binary archive and its signature
wget https://github.com/eigenwallet/core/releases/download/3.0.7/asb_3.0.7_Linux_x86_64.tar
wget https://github.com/eigenwallet/core/releases/download/3.0.7/asb_3.0.7_Linux_x86_64.tar.asc

# Verify the signature
gpg --verify asb_3.0.7_Linux_x86_64.tar.asc asb_3.0.7_Linux_x86_64.tar
```

Successful verification shows:

```
gpg: Signature made [date]
gpg: Good signature from "..."
```

The warning `This key is not certified with a trusted signature` is expected unless you've explicitly trusted the key.
