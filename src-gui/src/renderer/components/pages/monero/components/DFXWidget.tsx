import { Box, Dialog, DialogTitle, Button, DialogContent } from "@mui/material";
import DFXSwissLogo from "assets/dfx-logo.svg";
import { useState } from "react";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { dfxAuthenticate } from "renderer/rpc";

function DFXLogo({ height = 24 }: { height?: number }) {
  return (
    <Box sx={{ backgroundColor: "white", borderRadius: 1, display: "flex", alignItems: "center", padding: 1, height }}>
      <img src={DFXSwissLogo} alt="DFX Swiss" style={{ height: "100%", flex: 1 }} />
    </Box>
  );
}

// Component for DFX button and modal
export default function DfxButton() {
  const [dfxUrl, setDfxUrl] = useState<string | null>(null);

  const handleOpenDfx = async () => {
    // Get authentication token and URL (this will initialize DFX if needed)
    const response = await dfxAuthenticate();
    setDfxUrl(response.kyc_url);
    return response;
  };

  const handleCloseModal = () => {
    setDfxUrl(null);
  };

  return (
    <>
      <PromiseInvokeButton
        variant="outlined"
        size="small"
        startIcon={<DFXLogo height={12} />}
        onInvoke={handleOpenDfx}
        displayErrorSnackbar={true}
      >
        Fiat Conversion
      </PromiseInvokeButton>

      <Dialog
        open={dfxUrl != null}
        onClose={handleCloseModal}
        maxWidth="lg"
        fullWidth
      >
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
      </Dialog>
    </>
  );
}