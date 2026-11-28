import { Box, Tooltip } from "@mui/material";
import { ReactNode, useState } from "react";

type Props = {
  content: string;
  children: ReactNode;
  showTooltip?: boolean;
};

export default function ClickToCopy({
  content,
  children,
  showTooltip = true,
}: Props) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const wrapper = (
    <Box
      onClick={handleCopy}
      sx={{ cursor: "pointer", display: "inline-block" }}
    >
      {children}
    </Box>
  );

  if (!showTooltip) {
    return wrapper;
  }

  return (
    <Tooltip title={copied ? "Copied!" : "Click to copy"} arrow>
      {wrapper}
    </Tooltip>
  );
}
