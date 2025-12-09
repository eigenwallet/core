import Link from "@mui/material/Link";
import { openUrl } from "@tauri-apps/plugin-opener";

export default function ExternalLink({
  children,
  href,
}: {
  children: React.ReactNode;
  href: string;
}) {
  return (
    <Link style={{ cursor: "pointer" }} onClick={() => openUrl(href)}>
      {children}
    </Link>
  );
}
