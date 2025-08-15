import { Box, Drawer, Typography } from "@mui/material";
import QRCode from "react-qr-code";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import { useState } from "react";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";

export default function ReceiveButton({ address, disabled }: { address: string, disabled: boolean }) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <TextIconButton label="Receive" onClick={() => setOpen(true)} disabled={disabled}>
        <ArrowDownwardIcon />
      </TextIconButton>
      <Drawer open={open} onClose={() => setOpen(false)} anchor="bottom">
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 2,
            p: 2,
            pb: 8,
          }}
        >
          <Typography variant="h6">Receive Monero</Typography>
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 2,
              p: 2,
              borderRadius: 2,
              backgroundColor: "#fff",
            }}
          >
            <QRCode value={address} size={200} />
          </Box>
          <ActionableMonospaceTextBox content={address} enableQrCode={false} />
        </Box>
      </Drawer>
    </>
  );
}
