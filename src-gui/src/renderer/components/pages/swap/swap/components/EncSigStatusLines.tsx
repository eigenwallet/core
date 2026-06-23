import { Box, CircularProgress, Typography } from "@mui/material";
import { ReactNode } from "react";

export function StatusLine({ done, label }: { done: boolean; label: string }) {
  return (
    <Typography
      variant="caption"
      color="textSecondary"
      component="div"
      sx={{ display: "flex", alignItems: "center", gap: 0.5 }}
    >
      {done ? "✓" : <CircularProgress size={10} />}
      {label}
    </Typography>
  );
}

/// Horizontally centers the status lines while keeping them left-aligned so
/// every line starts at the same x coordinate.
export function StatusLines({ children }: { children: ReactNode }) {
  return (
    <Box sx={{ paddingTop: 0.5, display: "flex", justifyContent: "center" }}>
      <Box
        sx={{
          display: "inline-flex",
          flexDirection: "column",
          alignItems: "flex-start",
        }}
      >
        {children}
      </Box>
    </Box>
  );
}
