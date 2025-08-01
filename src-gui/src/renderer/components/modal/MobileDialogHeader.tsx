import { IconButton, Toolbar, Typography, AppBar } from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import { useIsMobile } from "../../../utils/useIsMobile";

interface MobileDialogHeaderProps {
  title: string;
  onClose: () => void;
}

export default function MobileDialogHeader({
  title,
  onClose,
}: MobileDialogHeaderProps) {
  const isMobile = useIsMobile();

  if (!isMobile) {
    return null;
  }

  return (
    <AppBar sx={{ position: "relative" }}>
      <Toolbar>
        <Typography sx={{ ml: 2, flex: 1 }} variant="h6" component="div">
          {title}
        </Typography>
        <IconButton
          edge="end"
          color="inherit"
          onClick={onClose}
          aria-label="close"
        >
          <CloseIcon />
        </IconButton>
      </Toolbar>
    </AppBar>
  );
}
