import {
  Box,
  CircularProgress,
  LinearProgress,
  Typography,
} from "@mui/material";
import { ReactNode } from "react";

export default function CircularProgressWithSubtitle({
  description,
  hideSpinner = false,
}: {
  description: string | ReactNode;
  hideSpinner?: boolean;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
        flexDirection: "column",
      }}
    >
      {!hideSpinner && <CircularProgress size={50} />}
      <Typography
        variant="subtitle2"
        sx={{ paddingTop: 1, textAlign: "center" }}
      >
        {description}
      </Typography>
    </Box>
  );
}

export function LinearProgressWithSubtitle({
  description,
  value,
}: {
  description: string | ReactNode;
  value: number;
}) {
  return (
    <Box
      style={{ gap: "0.5rem" }}
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <Typography variant="subtitle2" sx={{ paddingTop: 1 }}>
        {description}
      </Typography>
      <Box
        sx={{
          width: "10rem",
        }}
      >
        <LinearProgress
          variant={value === 100 ? "indeterminate" : "determinate"}
          value={value}
        />
      </Box>
    </Box>
  );
}
