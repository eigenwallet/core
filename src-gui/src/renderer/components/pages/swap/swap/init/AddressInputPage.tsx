import {
  Box,
  Paper,
  Typography,
  IconButton,
  InputAdornment,
  Switch,
  FormControlLabel,
} from "@mui/material";
import ClearIcon from "@mui/icons-material/Clear";
import { useState } from "react";
import BitcoinAddressTextField from "renderer/components/inputs/BitcoinAddressTextField";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import MakerOfferItem from "renderer/components/pages/swap/swap/init/deposit_and_choose_offer/MakerOfferItem";
import { usePendingSelectOfferApproval } from "store/hooks";

// Custom hook to manage address input state and validation
export function useAddressInputState() {
  const [redeemAddress, setRedeemAddress] = useState("");
  const [refundAddress, setRefundAddress] = useState("");
  const [redeemAddressValid, setRedeemAddressValid] = useState(true);
  const [refundAddressValid, setRefundAddressValid] = useState(true);

  const isValid =
    (redeemAddress.trim() === "" || redeemAddressValid) &&
    (refundAddress.trim() === "" || refundAddressValid);

  return {
    redeemAddress,
    setRedeemAddress,
    refundAddress,
    setRefundAddress,
    redeemAddressValid,
    setRedeemAddressValid,
    refundAddressValid,
    setRefundAddressValid,
    isValid,
  };
}

interface AddressInputPageProps {
  redeemAddress: string;
  setRedeemAddress: (value: string) => void;
  refundAddress: string;
  setRefundAddress: (value: string) => void;
  redeemAddressValid: boolean;
  setRedeemAddressValid: (value: boolean) => void;
  refundAddressValid: boolean;
  setRefundAddressValid: (value: boolean) => void;
}

export default function AddressInputPage({
  redeemAddress,
  setRedeemAddress,
  refundAddress,
  setRefundAddress,
  redeemAddressValid,
  setRedeemAddressValid,
  refundAddressValid,
  setRefundAddressValid,
}: AddressInputPageProps) {
  const pendingSelectOfferApprovals = usePendingSelectOfferApproval();
  const specifyRedeemRefundApproval = pendingSelectOfferApprovals[0];

  // Independent switch states
  const [useInternalRedeemWallet, setUseInternalRedeemWallet] = useState(true);
  const [useInternalRefundWallet, setUseInternalRefundWallet] = useState(true);

  const handleRedeemSwitchChange = (useInternal: boolean) => {
    setUseInternalRedeemWallet(useInternal);
    if (useInternal) {
      setRedeemAddress("");
      setRedeemAddressValid(true);
    } else {
      setRedeemAddressValid(false);
    }
  };

  const handleRefundSwitchChange = (useInternal: boolean) => {
    setUseInternalRefundWallet(useInternal);
    if (useInternal) {
      setRefundAddress("");
      setRefundAddressValid(true);
    } else {
      setRefundAddressValid(false);
    }
  };

  return (
    <>
      {specifyRedeemRefundApproval && (
        <Box>
          <MakerOfferItem
            quoteWithAddress={specifyRedeemRefundApproval.request.content.maker}
            requestId={undefined}
            noButton={true}
          />
        </Box>
      )}
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 1.5,
        }}
      >
        <Paper variant="outlined" sx={{ p: 2 }}>
          <Typography variant="h6" gutterBottom>
            Monero Redeem Address
          </Typography>
          <FormControlLabel
            control={
              <Switch
                checked={useInternalRedeemWallet}
                onChange={(e) => handleRedeemSwitchChange(e.target.checked)}
              />
            }
            label="Send Monero into your eigenwallet"
            sx={{ mb: useInternalRedeemWallet ? 0 : 2 }}
          />
          {!useInternalRedeemWallet && (
            <MoneroAddressTextField
              label="Custom redeem address"
              placeholder="Enter Monero address"
              address={redeemAddress}
              onAddressChange={setRedeemAddress}
              onAddressValidityChange={setRedeemAddressValid}
              helperText="Monero will be sent to this external address if the swap is successful"
              fullWidth
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">
                    <IconButton
                      size="small"
                      onClick={() => handleRedeemSwitchChange(true)}
                      title="Use internal wallet instead"
                    >
                      <ClearIcon fontSize="small" />
                    </IconButton>
                  </InputAdornment>
                ),
              }}
            />
          )}
        </Paper>

        <Paper variant="outlined" sx={{ p: 2 }}>
          <Typography variant="h6" gutterBottom>
            Bitcoin Refund Address
          </Typography>
          <FormControlLabel
            control={
              <Switch
                checked={useInternalRefundWallet}
                onChange={(e) => handleRefundSwitchChange(e.target.checked)}
              />
            }
            label="Send Bitcoin refunds into your eigenwallet"
            sx={{ mb: useInternalRefundWallet ? 0 : 2 }}
          />
          {!useInternalRefundWallet && (
            <BitcoinAddressTextField
              label="Custom refund address"
              placeholder="Enter Bitcoin address"
              address={refundAddress}
              onAddressChange={setRefundAddress}
              onAddressValidityChange={setRefundAddressValid}
              helperText="In case something goes wrong, Bitcoin will be refunded to this external address"
              fullWidth
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">
                    <IconButton
                      size="small"
                      onClick={() => handleRefundSwitchChange(true)}
                      title="Use internal wallet instead"
                    >
                      <ClearIcon fontSize="small" />
                    </IconButton>
                  </InputAdornment>
                ),
              }}
            />
          )}
        </Paper>
      </Box>
    </>
  );
}
