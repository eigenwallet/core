import React from "react";
import { Typography } from "@mui/material";
import type { TypographyProps } from "@mui/material";
import { keyframes } from "@mui/system";

export interface ShimmerTypographyProps extends TypographyProps {
  active?: boolean;
  durationMs?: number;
}

const shimmerKeyframes = keyframes`
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
`;

export default function ShimmerTypography({
  active = true,
  durationMs = 3600,
  sx,
  ...props
}: ShimmerTypographyProps) {
  return (
    <Typography
      {...props}
      sx={{
        ...(sx || {}),
        ...(active
          ? {
              background:
                "linear-gradient(90deg, rgba(255,255,255,0.3) 0%, rgba(255,255,255,0.8) 50%, rgba(255,255,255,0.3) 100%)",
              backgroundSize: "200% 100%",
              WebkitBackgroundClip: "text",
              backgroundClip: "text",
              color: "transparent",
              animation: `${shimmerKeyframes} ${durationMs}ms linear infinite`,
              opacity: 0.8,
            }
          : {}),
      }}
    />
  );
}


