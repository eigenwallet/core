import { Box, Button, IconButton, Tooltip } from "@mui/material";
import { FileCopyOutlined, QrCode as QrCodeIcon } from "@mui/icons-material";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useState } from "react";
import MonospaceTextBox from "./MonospaceTextBox";
import { Modal } from "@mui/material";
import QRCode from "react-qr-code";

type ModalProps = {
  open: boolean;
  onClose: () => void;
  content: string;
};

type Props = {
  content: string | null;
  displayCopyIcon?: boolean;
  enableQrCode?: boolean;
  light?: boolean;
  spoilerText?: string;
};

function QRCodeModal({ open, onClose, content }: ModalProps) {
  return (
    <Modal open={open} onClose={onClose}>
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 2,
          justifyContent: "center",
          alignItems: "center",
          height: "100%",
        }}
      >
        <QRCode
          value={content}
          size={500}
          style={{
            maxWidth: "90%",
            maxHeight: "90%",
            backgroundColor: "white",
            borderRadius: 2,
            padding: "1rem",
            border: "1rem solid #e0e0e0",
          }}
          viewBox="0 0 500 500"
        />
        <Button
          onClick={onClose}
          size="large"
          variant="contained"
          color="primary"
        >
          Done
        </Button>
      </Box>
    </Modal>
  );
}

export default function ActionableMonospaceTextBox({
  content,
  displayCopyIcon = true,
  enableQrCode = true,
  light = false,
  spoilerText,
}: Props) {
  const [copied, setCopied] = useState(false);
  const [qrCodeOpen, setQrCodeOpen] = useState(false);
  const [isQrCodeButtonHovered, setIsQrCodeButtonHovered] = useState(false);
  const [isRevealed, setIsRevealed] = useState(!spoilerText);

  const handleCopy = async () => {
    if (!content) return;
    await writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <>
      <Box sx={{ position: "relative" }}>
        <Tooltip
          title={
            isQrCodeButtonHovered
              ? ""
              : copied
                ? "Copied to clipboard"
                : "Click to copy"
          }
          arrow
        >
          <Box
            sx={{
              cursor: "pointer",
              filter: spoilerText && !isRevealed ? "blur(8px)" : "none",
              transition: "filter 0.3s ease",
            }}
            onClick={handleCopy}
          >
            <MonospaceTextBox
              light={light}
              actions={
                <>
                  {displayCopyIcon && (
                    <Tooltip title="Copy to clipboard" arrow>
                      <IconButton onClick={handleCopy} size="small">
                        <FileCopyOutlined />
                      </IconButton>
                    </Tooltip>
                  )}
                  {enableQrCode && (
                    <Tooltip title="Show QR Code" arrow>
                      <IconButton
                        onClick={(e) => {
                          e.stopPropagation();
                          setQrCodeOpen(true);
                        }}
                        onMouseEnter={() => setIsQrCodeButtonHovered(true)}
                        onMouseLeave={() => setIsQrCodeButtonHovered(false)}
                        size="small"
                      >
                        <QrCodeIcon />
                      </IconButton>
                    </Tooltip>
                  )}
                </>
              }
            >
              {content}
            </MonospaceTextBox>
          </Box>
        </Tooltip>

        {spoilerText && !isRevealed && (
          <Box
            onClick={() => setIsRevealed(true)}
            sx={{
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              cursor: "pointer",
              bgcolor: "rgba(0, 0, 0, 0.1)",
              borderRadius: 1,
            }}
          >
            <Box
              sx={{
                bgcolor: "background.paper",
                p: 2,
                borderRadius: 1,
                boxShadow: 2,
              }}
            >
              {spoilerText}
            </Box>
          </Box>
        )}
      </Box>

      {enableQrCode && content && (
        <QRCodeModal
          open={qrCodeOpen}
          onClose={() => setQrCodeOpen(false)}
          content={content}
        />
      )}
    </>
  );
}
