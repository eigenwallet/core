import React from "react";
import { Badge } from "@mui/material";
import {
  useRunningSwapsCount,
  useResumeableSwapsCountExcludingPunished,
} from "store/hooks";

export default function UnfinishedSwapsBadge({
  children,
}: {
  children: React.ReactNode;
}) {
  const runningSwapsCount = useRunningSwapsCount();
  const resumableSwapsCount = useResumeableSwapsCountExcludingPunished();

  const displayedResumableSwapsCount = Math.max(
    0,
    resumableSwapsCount - runningSwapsCount,
  );

  if (displayedResumableSwapsCount > 0) {
    return (
      <Badge badgeContent={displayedResumableSwapsCount} color="primary">
        {children}
      </Badge>
    );
  }
  return children;
}
