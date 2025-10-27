import { DialogContentText } from "@mui/material";
import BitcoinAddressTextField from "../../../inputs/BitcoinAddressTextField";

export default function AddressInputPage({
  withdrawAddress,
  setWithdrawAddress,
  setWithdrawAddressValid,
}: {
  withdrawAddress: string;
  setWithdrawAddress: (address: string) => void;
  setWithdrawAddressValid: (valid: boolean) => void;
}) {
  return (
    <>
      <DialogContentText>
        To withdraw the Bitcoin inside the internal wallet, please enter an
        address. All funds (the entire balance) will be sent to that address.
      </DialogContentText>

      <BitcoinAddressTextField
        address={withdrawAddress}
        onAddressChange={setWithdrawAddress}
        onAddressValidityChange={setWithdrawAddressValid}
        helperText="All Bitcoin of the internal wallet will be transferred to this address"
        allowEmpty={false}
        fullWidth
      />
    </>
  );
}
