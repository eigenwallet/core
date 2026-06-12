import { Link } from "@mui/material";

export default function DebugPageSwitchBadge({
  enabled,
  setEnabled,
}: {
  enabled: boolean;
  setEnabled: (enabled: boolean) => void;
}) {
  return (
    <Link
      component="button"
      type="button"
      onClick={() => setEnabled(!enabled)}
      variant="caption"
      color={enabled ? "primary" : "text.secondary"}
      underline="hover"
    >
      Debug
    </Link>
  );
}
