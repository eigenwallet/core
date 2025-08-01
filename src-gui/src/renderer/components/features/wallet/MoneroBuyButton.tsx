import { Box, Drawer, Typography } from "@mui/material";
import QRCode from "react-qr-code";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import { useState } from "react";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import MobileDialog from "renderer/components/modal/MobileDialog";
import MobileDialogHeader from "renderer/components/modal/MobileDialogHeader";

export default function ReceiveButton({ address }: { address: string }) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <TextIconButton label="Receive" onClick={() => setOpen(true)}>
        <ArrowDownwardIcon />
      </TextIconButton>
      <MobileDialog open={open} onClose={() => setOpen(false)}>
        <MobileDialogHeader title="Buy Monero" onClose={() => setOpen(false)} />
      </MobileDialog>
    </>
  );
}
