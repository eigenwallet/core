import { Box } from "@mui/material";
import MoneroRawTransactionButton from "renderer/components/other/MoneroRawTransactionButton";
import { captionLinkSx } from "renderer/components/other/captionLinkSx";
import { SwapMoneroRecoveryButton } from "renderer/components/pages/history/table/SwapMoneroRecoveryButton";
import { useActiveSwapInfo } from "store/hooks";
import CircularProgressWithSubtitle from "../components/CircularProgressWithSubtitle";

export default function PublishingMoneroRedeemPage({
  xmr_redeem_tx_hex,
}: {
  xmr_redeem_tx_hex: string;
}) {
  const swap = useActiveSwapInfo();

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 1,
      }}
    >
      <CircularProgressWithSubtitle description="Publishing the Monero redeem transaction" />
      {swap && (
        <SwapMoneroRecoveryButton swap={swap} variant="text" sx={captionLinkSx}>
          Redeem manually
        </SwapMoneroRecoveryButton>
      )}
      <MoneroRawTransactionButton txHex={xmr_redeem_tx_hex} />
    </Box>
  );
}
