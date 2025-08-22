import { Typography, useTheme } from "@mui/material";
import type { TypographyProps } from "@mui/material";
import { alpha, keyframes } from "@mui/system";

export interface ShimmerTypographyProps extends TypographyProps {
  active?: boolean;
  durationMs?: number;
}

const shimmerKeyframes = keyframes`
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
`;

function resolveColor(path: string, theme) {
  const colorPalette = path.split('.').reduce((obj, key) => obj?.[key], theme.palette);
  return typeof colorPalette === 'string' ? colorPalette : colorPalette.main;
}

export default function ShimmerTypography({
  active = true,
  durationMs = 3600,
  sx,
  color = "text.primary",
  ...props
}: ShimmerTypographyProps) {

  const theme = useTheme();
  
  const hexColor = resolveColor(color, theme);

  return (
    <Typography
      {...props}
      sx={{
        ...(sx || {}),
        ...(active
          ? {
              background:
                `linear-gradient(90deg, ${alpha(hexColor, 0.3)} 0%, ${alpha(hexColor, 1)} 50%, ${alpha(hexColor, 0.3)} 100%)`,
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


