import { Box } from "@mui/material";
import CircularProgressWithSubtitle from "../components/CircularProgressWithSubtitle";
import { StatusLine, StatusLines } from "../components/EncSigStatusLines";

export default function EncryptedSignatureSentPage({
  hermes_used,
}: {
  hermes_used: boolean;
}) {
  return (
    <CircularProgressWithSubtitle
      description={
        <Box>
          Waiting for them to redeem the Bitcoin
          <StatusLines>
            <StatusLine done label="Sent over network" />
            {hermes_used && (
              <StatusLine done label="On-chain Hermes message confirmed" />
            )}
          </StatusLines>
        </Box>
      }
    />
  );
}
