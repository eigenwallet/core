import { Box, DialogContentText } from "@mui/material";
import { TauriSwapProgressEvent } from "models/tauriModel";
import CliLogsBox from "renderer/components/other/RenderedCliLog";
import { useActiveSwapInfo, useActiveSwapLogs } from "store/hooks";
import SwapStatePage from "renderer/components/pages/swap/swap/SwapStatePage";
import { logsToRawString } from "utils/parseUtils";

export default function ProcessExitedPage({
  prevState,
  swapId,
}: {
  prevState: TauriSwapProgressEvent | null;
  swapId: string;
}) {
  const swap = useActiveSwapInfo();
  const logs = useActiveSwapLogs();

  if (
    prevState != null &&
    (prevState.type === "XmrRedeemInMempool" ||
      prevState.type === "BtcRefunded" ||
      prevState.type === "BtcPunished" ||
      prevState.type === "CooperativeRedeemRejected")
  ) {
    return (
      <SwapStatePage
        state={{
          curr: prevState,
          prev: null,
          swapId,
        }}
      />
    );
  }

  const logEntries = logs.map(({ log }) => log);

  return (
    <>
      <DialogContentText>
        The swap was stopped but it has not been completed yet. Check the logs
        below for more information. The current GUI state is{" "}
        {prevState?.type ?? "unknown"}. The current database state is{" "}
        {swap?.state_name ?? "unknown"}.
      </DialogContentText>
      <Box>
        <CliLogsBox logs={logs} label="Logs relevant to the swap" />
      </Box>
    </>
  );
}
