import React from "react";
import { Box, useTheme } from "@mui/material";
import { keyframes } from "@mui/system";
import Avatar from "boring-avatars";

export interface AvatarWithProgressProps {
  size?: number;
  src?: string;
  alt?: string;
  isLoading?: boolean;
  progress?: number | null;
  ringWidth?: number;
  onClick?: () => void;
  gradientSeed?: string;
}

const pulseOpacity = keyframes`
  0% { opacity: 0.5; }
  50% { opacity: 1; }
  100% { opacity: 0.5; }
`;

/**
 * Circular avatar with an Instagram-like gradient ring that doubles as a circular progress bar.
 * - isLoading: pulsates the ring opacity
 * - progress: 0..1 determines how filled the ring is (defaults to full)
 */
export default function AvatarWithProgress({
  size = 56,
  src,
  alt,
  isLoading = false,
  progress,
  ringWidth = 3,
  gradientSeed,
  onClick,
}: AvatarWithProgressProps) {
    const theme = useTheme();
  const clampedProgress = typeof progress === "number" && !Number.isNaN(progress)
    ? Math.max(0, Math.min(1, progress))
    : 1; // full ring by default

  const radius = (size - ringWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - clampedProgress);

  return (
    <Box
      onClick={onClick}
      sx={{
        position: "relative",
        width: size,
        height: size,
        display: "inline-block",
      }}
    >
      {/* Progress ring */}
      <Box
        component="svg"
        viewBox={`0 0 ${size} ${size}`}
        sx={{
          position: "absolute",
          inset: 0,
          transform: "rotate(90deg)", // start at bottom-center
          animation: isLoading ? `${pulseOpacity} 1.6s ease-in-out infinite` : "none",
        }}
      >
        <defs>
          {/* Instagram-like gradient */}
          <linearGradient id="igGradient" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor="#00FFC2" />
            <stop offset="100%" stopColor="#004F3B" />
          </linearGradient>
        </defs>

        {/* Track (subtle) */}
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="rgba(255,255,255,0.18)"
          strokeWidth={ringWidth}
          fill="none"
        />

        {/* Progress */}
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          stroke="url(#igGradient)"
          strokeLinecap="round"
          strokeWidth={ringWidth}
          fill="none"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          style={{ transition: "stroke-dashoffset 600ms ease" }}
        />
      </Box>

      {/* Avatar content */}
      <Box
        sx={{
          position: "absolute",
          inset: ringWidth + 3, // small inset so ring stays outside
          borderRadius: "50%",
          overflow: "hidden",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "transparent",
        }}
      >
        <Avatar
          size={size}
          name={gradientSeed}
          variant="marble"
          colors={["#00FFC2", "#004F3B"]}
        />
      </Box>
    </Box>
  );
}


