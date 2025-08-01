import {
  Box,
  Dialog,
  DialogTitle,
  Button,
  DialogContent,
  Chip,
  Tooltip,
} from "@mui/material";
import { EuroSymbol as EuroIcon } from "@mui/icons-material";
import DFXSwissLogo from "assets/dfx-logo.svg";
import { useState } from "react";
import { dfxAuthenticate } from "renderer/rpc";
import MobileDialog from "renderer/components/modal/MobileDialog";
import TextIconButton from "renderer/components/buttons/TextIconButton";
import { useIsMobile } from "utils/useIsMobile";

function DFXLogo({ height = 24 }: { height?: number }) {
  return (
    <Box
      sx={{
        backgroundColor: "white",
        borderRadius: 1,
        display: "flex",
        alignItems: "center",
        padding: 1,
        height,
      }}
    >
      <img
        src={DFXSwissLogo}
        alt="DFX Swiss"
        style={{ height: "100%", flex: 1 }}
      />
    </Box>
  );
}

// Component for DFX button and modal
export default function DfxButton() {
  const [dfxUrl, setDfxUrl] = useState<string | null>(null);

  const handleOpenDfx = async () => {
    try {
      // Get authentication token and URL (this will initialize DFX if needed)
      const response = await dfxAuthenticate();
      setDfxUrl(response.kyc_url);
      return response;
    } catch (error) {
      console.error("DFX authentication failed:", error);
      // TODO: Show error snackbar if needed
      throw error;
    }
  };

  const handleCloseModal = () => {
    setDfxUrl(null);
  };

  const content = (
    <>
      <DialogTitle>
        <Box
          sx={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <DFXLogo />
          <Button onClick={handleCloseModal} variant="outlined">
            Close
          </Button>
        </Box>
      </DialogTitle>
      <DialogContent sx={{ p: 0, height: "min(40rem, 80vh)" }}>
        {dfxUrl && (
          <iframe
            src={dfxUrl}
            style={{
              width: "100%",
              height: "100%",
              border: "none",
            }}
            title="DFX Swiss"
          />
        )}
      </DialogContent>
    </>
  );

  if (useIsMobile()) {
    return <DFXWidgetMobile open={dfxUrl != null} onOpen={handleOpenDfx} onClose={handleCloseModal}>
      {content}
    </DFXWidgetMobile>;
  } else {
    return (
      <DFXWidgetDesktop
        handleOpenDfx={handleOpenDfx}
        handleCloseModal={handleCloseModal}
        open={dfxUrl != null}
      >
        {content}
      </DFXWidgetDesktop>
    );
  }
}

function DFXWidgetMobile({
  children,
  open,
  onOpen,
  onClose,
}: {
  children: React.ReactNode;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <TextIconButton label="Buy" onClick={onOpen}>
        <EuroIcon />
      </TextIconButton>
      <MobileDialog open={open} onClose={onClose}>
        {children}
      </MobileDialog>
    </>
  );
}

function DFXWidgetDesktop({
  handleOpenDfx,
  handleCloseModal,
  open,
  children,
}: {
  handleOpenDfx: () => void;
  handleCloseModal: () => void;
  open: boolean;
  children: React.ReactNode;
}) {
  return (
    <>
      <Tooltip title="Buy Monero with fiat using DFX" enterDelay={500}>
        <Chip
          variant="button"
          icon={<EuroIcon />}
          label="Buy Monero"
          clickable
          onClick={handleOpenDfx}
        />
      </Tooltip>

      <Dialog
        open={open}
        onClose={handleCloseModal}
        maxWidth="lg"
        fullWidth
      >
        {children}
      </Dialog>
    </>
  );
}
