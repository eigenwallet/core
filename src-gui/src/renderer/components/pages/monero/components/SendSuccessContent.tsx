import { Box, Button, Typography } from "@mui/material";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import {
  FiatPiconeroAmount,
  PiconeroAmount,
} from "renderer/components/other/Units";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import ArrowOutwardIcon from "@mui/icons-material/ArrowOutward";
import { SendMoneroResponse } from "models/tauriModel";
import { getMoneroTxExplorerUrl } from "../../../../../utils/conversionUtils";
import { isTestnet } from "store/config";
import { openUrl } from "@tauri-apps/plugin-opener";

export default function SendSuccessContent({
  onClose,
  successDetails,
}: {
  onClose: (event: unknown, reason: string) => void;
  successDetails: SendMoneroResponse | null;
}) {
  const address = successDetails?.address;
  const amount = successDetails?.amount_sent;
  const explorerUrl = successDetails?.tx_hash
    ? getMoneroTxExplorerUrl(successDetails.tx_hash, isTestnet())
    : null;

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        justifyContent: "center",
        alignItems: "center",
        minHeight: "400px",
        minWidth: "500px",
        gap: 7,
        p: 4,
      }}
    >
      <CheckCircleIcon sx={{ fontSize: 64, mt: 3 }} />
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 1,
        }}
      >
        <Typography variant="h4">Transaction Published</Typography>
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            alignItems: "center",
            gap: 1,
          }}
        >
          <Typography variant="body1" color="text.secondary">
            Sent
          </Typography>
          <Typography variant="body1" color="text.primary">
            <PiconeroAmount amount={amount} fixedPrecision={4} />
          </Typography>
          <Typography variant="body1" color="text.secondary">
            (<FiatPiconeroAmount amount={amount} />)
          </Typography>
        </Box>
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            alignItems: "center",
            gap: 1,
          }}
        >
          <Typography variant="body1" color="text.secondary">
            to
          </Typography>
          <Typography variant="body1" color="text.primary">
            <MonospaceTextBox>
              {address ? `${address.slice(0, 8)}...${address.slice(-8)}` : "?"}
            </MonospaceTextBox>
          </Typography>
        </Box>
      </Box>
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 1,
        }}
      >
        <Button onClick={onClose} variant="contained" color="primary">
          Done
        </Button>
        <Button
          color="primary"
          size="small"
          disabled={explorerUrl == null}
          endIcon={<ArrowOutwardIcon />}
          onClick={() => {
            if (explorerUrl != null) {
              openUrl(explorerUrl);
            }
          }}
        >
          View on Explorer
        </Button>
      </Box>
    </Box>
  );
}
