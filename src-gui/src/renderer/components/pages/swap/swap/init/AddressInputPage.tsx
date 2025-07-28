import { Box, Paper, Tab, Tabs, Typography } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import { useState } from "react";
import BitcoinAddressTextField from "renderer/components/inputs/BitcoinAddressTextField";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { usePendingSpecifyRedeemRefundApproval, useSettings } from "store/hooks";
import { resolveApproval } from "renderer/rpc";
import { LabeledMoneroAddress } from "models/tauriModel";
import { isTestnet } from "store/config";

// Donation addresses
const DONATION_ADDRESS_MAINNET = "49LEH26DJGuCyr8xzRAzWPUryzp7bpccC7Hie1DiwyfJEyUKvMFAethRLybDYrFdU1eHaMkKQpUPebY4WT3cSjEvThmpjPa";
const DONATION_ADDRESS_STAGENET = "56E274CJxTyVuuFG651dLURKyneoJ5LsSA5jMq4By9z9GBNYQKG8y5ejTYkcvZxarZW6if14ve8xXav2byK4aRnvNdKyVxp";

export default function AddressInputPage() {
  const pendingSpecifyRedeemRefundApprovals = usePendingSpecifyRedeemRefundApproval();
  const specifyRedeemRefundApproval = pendingSpecifyRedeemRefundApprovals[0]; // Assuming there's only one at a time

  const [redeemAddress, setRedeemAddress] = useState("");
  const [refundAddress, setRefundAddress] = useState("");
  const [useExternalRefundAddress, setUseExternalRefundAddress] =
    useState(false);

  // We force this to true for now because the internal wallet is not really accessible from the GUI yet
  const [useExternalRedeemAddress, setUseExternalRedeemAddress] =
    useState(true);

  const [redeemAddressValid, setRedeemAddressValid] = useState(false);
  const [refundAddressValid, setRefundAddressValid] = useState(false);

  const donationRatio = useSettings((s) => s.donateToDevelopment);

  async function confirmOffer() {
    if (!specifyRedeemRefundApproval) return;

    const address_pool: LabeledMoneroAddress[] = [];
    if (donationRatio !== false) {
      const donation_address = isTestnet()
        ? DONATION_ADDRESS_STAGENET
        : DONATION_ADDRESS_MAINNET;

      address_pool.push(
        {
          address: useExternalRedeemAddress ? redeemAddress : "internal",
          percentage: 1 - donationRatio,
          label: "Your wallet",
        },
        {
          address: donation_address,
          percentage: donationRatio,
          label: "Tip to the developers",
        },
      );
    } else {
      address_pool.push({
        address: useExternalRedeemAddress ? redeemAddress : "internal",
        percentage: 1,
        label: "Your wallet",
      });
    }

    await resolveApproval(specifyRedeemRefundApproval.request_id, {
      bitcoin_change_address: useExternalRefundAddress ? refundAddress : null,
      monero_receive_pool: address_pool,
    });
  }

  return (
    <>
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 1.5,
        }}
      >
        <Paper variant="outlined" style={{}}>
          <Tabs
            value={useExternalRedeemAddress ? 1 : 0}
            indicatorColor="primary"
            variant="fullWidth"
            onChange={(_, newValue) =>
              setUseExternalRedeemAddress(newValue === 1)
            }
          >
            <Tab label="Redeem to internal Monero wallet" value={0} />
            <Tab label="Redeem to external Monero address" value={1} />
          </Tabs>
          <Box style={{ padding: "16px" }}>
            {useExternalRedeemAddress ? (
              <MoneroAddressTextField
                label="External Monero redeem address"
                address={redeemAddress}
                onAddressChange={setRedeemAddress}
                onAddressValidityChange={setRedeemAddressValid}
                helperText="The monero will be sent to this address if the swap is successful."
                fullWidth
              />
            ) : (
              <Typography variant="caption">
                The Monero will be sent to the internal Monero wallet of the
                GUI. You can then withdraw them from there or use them for
                another swap directly.
              </Typography>
            )}
          </Box>
        </Paper>

        <Paper variant="outlined" style={{}}>
          <Tabs
            value={useExternalRefundAddress ? 1 : 0}
            indicatorColor="primary"
            variant="fullWidth"
            onChange={(_, newValue) =>
              setUseExternalRefundAddress(newValue === 1)
            }
          >
            <Tab label="Refund to internal Bitcoin wallet" value={0} />
            <Tab label="Refund to external Bitcoin address" value={1} />
          </Tabs>
          <Box style={{ padding: "16px" }}>
            {useExternalRefundAddress ? (
              <BitcoinAddressTextField
                label="External Bitcoin refund address"
                address={refundAddress}
                onAddressChange={setRefundAddress}
                onAddressValidityChange={setRefundAddressValid}
                helperText="In case something goes wrong, the Bitcoin will be refunded to this address."
                fullWidth
              />
            ) : (
              <Typography variant="caption">
                In case something goes wrong, the Bitcoin will be refunded to
                the internal Bitcoin wallet of the GUI. You can then withdraw
                them from there or use them for another swap directly.
              </Typography>
            )}
          </Box>
        </Paper>
      </Box>
      <Box style={{ display: "flex", justifyContent: "center" }}>
        <PromiseInvokeButton
          disabled={
            (!refundAddressValid && useExternalRefundAddress) ||
            (!redeemAddressValid && useExternalRedeemAddress)
          }
          variant="contained"
          color="primary"
          size="large"
          sx={{ marginTop: 1 }}
          endIcon={<PlayArrowIcon />}
          onInvoke={confirmOffer}
          displayErrorSnackbar
        >
          Confirm
        </PromiseInvokeButton>
      </Box>
    </>
  );
}
