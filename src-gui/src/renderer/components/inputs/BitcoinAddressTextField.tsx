import TextField, { TextFieldProps } from "@mui/material/TextField";
import { useEffect } from "react";
import { isTestnet } from "store/config";
import { isBtcAddressValid } from "utils/conversionUtils";

type BitcoinAddressTextFieldProps = TextFieldProps & {
  address: string;
  onAddressChange: (address: string) => void;
  onAddressValidityChange?: (valid: boolean) => void;
  helperText?: string;
  allowEmpty?: boolean;
};

export default function BitcoinAddressTextField({
  address,
  onAddressChange,
  onAddressValidityChange,
  helperText,
  allowEmpty = true,
  ...props
}: BitcoinAddressTextFieldProps) {
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
    if (onAddressValidityChange != null) {
      onAddressValidityChange(!errorText());
    }
  }, [address, onAddressValidityChange]);

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
