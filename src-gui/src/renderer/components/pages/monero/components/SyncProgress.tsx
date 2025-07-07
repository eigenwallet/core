import {
  Box,
  Typography,
  LinearProgress,
  Card,
  CardContent,
  Stack,
} from "@mui/material";

interface SyncProgressProps {
  syncProgress?: {
    current_block: number;
    target_block: number;
    progress_percentage: number;
  };
}

// Component for displaying sync progress
export default function SyncProgress({ syncProgress }: SyncProgressProps) {
  if (!syncProgress) return null;

  return (
    <Card>
      <CardContent>
        <Stack spacing={1}>
          <Box
            sx={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <Typography variant="body2" color="text.secondary">
              Block {syncProgress.current_block.toLocaleString()} of{" "}
              {syncProgress.target_block.toLocaleString()}
            </Typography>
            <Typography variant="body2" color="text.secondary">
              {syncProgress.progress_percentage.toFixed(2)}%
            </Typography>
          </Box>
          <LinearProgress
            variant="determinate"
            value={syncProgress.progress_percentage}
            sx={{ height: 8, borderRadius: 4 }}
          />
          {syncProgress.progress_percentage < 100 && (
            <Typography variant="body2" color="text.secondary">
              Wallet is synchronizing with the Monero network...
            </Typography>
          )}
          {syncProgress.progress_percentage >= 100 && (
            <Typography variant="body2" color="success.main">
              Wallet is fully synchronized
            </Typography>
          )}
        </Stack>
      </CardContent>
    </Card>
  );
}
