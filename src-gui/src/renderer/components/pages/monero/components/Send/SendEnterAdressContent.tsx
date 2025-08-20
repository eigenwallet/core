import { Box, Button, Typography } from "@mui/material";
import { useState } from "react";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";

export default function SendEnterAddressContent({ onContinue, address, onAddressChange, onAddressValidityChange }: { onContinue: () => void, address: string, onAddressChange: (address: string) => void, onAddressValidityChange: (valid: boolean) => void }) {
  const [isValidAddress, setIsValidAddress] = useState(false);

  const handleValidityChange = (valid: boolean) => {
    setIsValidAddress(valid);
    onAddressValidityChange(valid);
  }
  
  return (
    <Box>
      <Typography variant="h6">Enter Address</Typography>
      <MoneroAddressTextField
        address={address}
        onAddressChange={onAddressChange}
        onAddressValidityChange={handleValidityChange}
        label="Send to"
        fullWidth
      />
      <Button onClick={onContinue} disabled={!isValidAddress}>Continue</Button>
    </Box>
  );
}