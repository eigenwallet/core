import { Box, Chip, Typography } from "@mui/material";
import CloudUploadIcon from "@mui/icons-material/CloudUpload";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import { ApolloSwapState } from "models/tauriModel";
import { useAppSelector } from "store/hooks";

const STATES: ApolloSwapState[] = [
  ApolloSwapState.Waiting,
  ApolloSwapState.Publishing,
  ApolloSwapState.Published,
  ApolloSwapState.Retrying,
  ApolloSwapState.Confirmed,
  ApolloSwapState.Abandoned,
];

export default function ApolloBackupsBox() {
  const swaps = useAppSelector((state) => state.apollo.status?.swaps ?? []);
  const unfinished = swaps.filter(
    (swap) =>
      swap.state !== ApolloSwapState.Confirmed &&
      swap.state !== ApolloSwapState.Abandoned,
  );

  return (
    <InfoBox
      title={
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <CloudUploadIcon />
          On-chain backups
        </Box>
      }
      mainContent={
        <Typography variant="subtitle2">
          Status of the encrypted on-chain backups of your swaps (Apollo).
        </Typography>
      }
      additionalContent={
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <Box sx={{ display: "flex", gap: 1, flexWrap: "wrap" }}>
            {STATES.map((state) => (
              <Chip
                key={state}
                size="small"
                variant="outlined"
                label={`${swaps.filter((swap) => swap.state === state).length} ${state}`}
              />
            ))}
          </Box>
          {unfinished.map((swap) => (
            <Typography key={swap.swap_id} variant="body2">
              {swap.swap_id}: {swap.state}
            </Typography>
          ))}
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
