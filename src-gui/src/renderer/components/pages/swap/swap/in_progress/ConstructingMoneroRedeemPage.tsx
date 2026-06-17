import { Box } from "@mui/material";
import { captionLinkSx } from "renderer/components/other/captionLinkSx";
import { SwapMoneroRecoveryButton } from "renderer/components/pages/history/table/SwapMoneroRecoveryButton";
import { useActiveSwapInfo } from "store/hooks";
import CircularProgressWithSubtitle from "../components/CircularProgressWithSubtitle";

export default function ConstructingMoneroRedeemPage() {
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
      <CircularProgressWithSubtitle description="Constructing the Monero redeem transaction" />
      {swap && (
        <SwapMoneroRecoveryButton swap={swap} variant="text" sx={captionLinkSx}>
          Redeem manually
        </SwapMoneroRecoveryButton>
      )}
    </Box>
  );
}
