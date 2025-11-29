import { Box, Button, Typography } from "@mui/material";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import {
  FiatPiconeroAmount,
  PiconeroAmount,
  FiatSatsAmount,
  SatsAmount,
} from "renderer/components/other/Units";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import ArrowOutwardIcon from "@mui/icons-material/ArrowOutward";
import { SendMoneroResponse, WithdrawBtcResponse } from "models/tauriModel";
import {
  getMoneroTxExplorerUrl,
  getBitcoinTxExplorerUrl,
} from "../../../../../utils/conversionUtils";
import { isTestnet } from "store/config";
import { open } from "@tauri-apps/plugin-shell";

export default function SendSuccessContent({
  onClose,
  successDetails,
  wallet,
}: {
  onClose: () => void;
  successDetails: SendMoneroResponse | WithdrawBtcResponse | null;
  wallet: "monero" | "bitcoin";
}) {
  const details = successDetails as
    | (SendMoneroResponse & WithdrawBtcResponse)
    | null;
  const address = details?.address;
  const amount = details?.amount_sent || details?.amount;
  const explorerUrl = details?.tx_hash
    ? getMoneroTxExplorerUrl(details.tx_hash, isTestnet())
    : details?.txid
      ? getBitcoinTxExplorerUrl(details.txid, isTestnet())
      : null;

  const BaseUnitAmount = wallet === "monero" ? PiconeroAmount : SatsAmount;
  const FiatBaseUnitAmount =
    wallet === "monero" ? FiatPiconeroAmount : FiatSatsAmount;
  const baseUnitPrecision = wallet === "monero" ? 4 : 6;

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
            <BaseUnitAmount
              amount={amount}
              fixedPrecision={baseUnitPrecision}
            />
          </Typography>
          <Typography variant="body1" color="text.secondary">
            (<FiatBaseUnitAmount amount={amount} />)
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
              open(explorerUrl);
            }
          }}
        >
          View on Explorer
        </Button>
      </Box>
    </Box>
  );
}
