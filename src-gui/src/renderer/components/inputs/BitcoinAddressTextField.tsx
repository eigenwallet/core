import TextField, { TextFieldProps } from "@mui/material/TextField";
import { useEffect, forwardRef } from "react";
import { isTestnet } from "store/config";
import { isBtcAddressValid } from "utils/conversionUtils";

const BitcoinAddressTextField = forwardRef<
  HTMLInputElement,
  {
    address: string;
    onAddressChange: (address: string) => void;
    onAddressValidityChange: (valid: boolean) => void;
    helperText: string;
  } & TextFieldProps
>(
  (
    { address, onAddressChange, onAddressValidityChange, helperText, ...props },
    ref,
  ) => {
    const placeholder = isTestnet() ? "tb1q4aelwalu..." : "bc18ociqZ9mZ...";
    const errorText = isBtcAddressValid(address, isTestnet())
      ? null
      : `Only bech32 addresses are supported. They begin with "${
          isTestnet() ? "tb1" : "bc1"
        }"`;

    useEffect(() => {
      onAddressValidityChange(!errorText);
    }, [address, errorText, onAddressValidityChange]);

    return (
      <TextField
        ref={ref}
        value={address}
        onChange={(e) => onAddressChange(e.target.value)}
        error={!!errorText && address.length > 0}
        helperText={address.length > 0 ? errorText || helperText : helperText}
        placeholder={placeholder}
        variant="outlined"
        {...props}
      />
    );
  },
);

BitcoinAddressTextField.displayName = "BitcoinAddressTextField";

export default BitcoinAddressTextField;
