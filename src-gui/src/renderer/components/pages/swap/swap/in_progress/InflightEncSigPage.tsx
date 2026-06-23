import { Box } from "@mui/material";
import { HermesProgressKind } from "models/tauriModel";
import CircularProgressWithSubtitle from "../components/CircularProgressWithSubtitle";
import { StatusLine, StatusLines } from "../components/EncSigStatusLines";

const HERMES_PROGRESS_LABEL: Record<HermesProgressKind, string | null> = {
  [HermesProgressKind.None]: null,
  [HermesProgressKind.Constructing]: "Constructing on-chain Hermes message",
  [HermesProgressKind.Constructed]: "Publishing on-chain Hermes message",
  [HermesProgressKind.Published]: "Publishing on-chain Hermes message",
  [HermesProgressKind.Confirmed]: "On-chain Hermes message confirmed",
};

export default function InflightEncSigPage({
  p2p_sent,
  hermes,
}: {
  p2p_sent: boolean;
  hermes: HermesProgressKind;
}) {
  // Once either channel has fully delivered the encrypted signature, the other
  // party can redeem Bitcoin and we are only waiting for them to do so.
  const delivered = p2p_sent || hermes === HermesProgressKind.Confirmed;

  const hermesLabel = HERMES_PROGRESS_LABEL[hermes];

  return (
    <CircularProgressWithSubtitle
      hideSpinner={!delivered}
      description={
        <Box>
          {delivered
            ? "Waiting for them to redeem Bitcoin"
            : "Sending encrypted signature to allow them to redeem Bitcoin"}
          <StatusLines>
            <StatusLine
              done={p2p_sent}
              label={p2p_sent ? "Sent over network" : "Sending over network..."}
            />
            {hermesLabel && (
              <StatusLine
                done={hermes === HermesProgressKind.Confirmed}
                label={hermesLabel}
              />
            )}
          </StatusLines>
        </Box>
      }
    />
  );
}
