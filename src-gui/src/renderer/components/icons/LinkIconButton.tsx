import { IconButton } from "@mui/material";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ReactNode } from "react";

export default function LinkIconButton({
  url,
  children,
}: {
  url: string;
  children: ReactNode;
}) {
  return (
    <IconButton component="span" onClick={() => openUrl(url)} size="large">
      {children}
    </IconButton>
  );
}
