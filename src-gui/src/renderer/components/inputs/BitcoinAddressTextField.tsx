import TextField, { TextFieldProps } from "@mui/material/TextField";
import { useEffect } from "react";
import { isTestnet } from "store/config";
import { isBtcAddressValid } from "utils/conversionUtils";

type BitcoinAddressTextFieldProps = {
  address: string;
  onAddressChange: (address: string) => void;
  helperText: string;
  onAddressValidityChange?: (valid: boolean) => void;
  allowEmpty?: boolean;
};

export default function BitcoinAddressTextField({
  address,
  onAddressChange,
  helperText,
  allowEmpty = true,
  onAddressValidityChange = () => {},
  ...props
}: BitcoinAddressTextFieldProps & TextFieldProps) {
  const placeholder = isTestnet() ? "tb1q4aelwalu..." : "bc18ociqZ9mZ...";

  function errorText() {
    if (address.length === 0) {
      if (allowEmpty) {
        return null;
      }

      return "Cannot be empty";
    }

    if (isBtcAddressValid(address, isTestnet())) {
      return null;
    }

    return "Not a valid Bitcoin address";
  }

  useEffect(() => {
    if (onAddressValidityChange) {
      onAddressValidityChange(!errorText());
    }
  }, [address, errorText, onAddressValidityChange]);

  return (
    <TextField
      value={address}
      onChange={(e) => onAddressChange(e.target.value)}
      error={errorText() !== null}
      helperText={errorText() || helperText}
      placeholder={placeholder}
      variant="outlined"
      {...props}
    />
  );
}
