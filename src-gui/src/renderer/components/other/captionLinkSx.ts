import { SxProps, Theme } from "@mui/material";

// Styles a MUI Button to read as a small secondary-color caption link so
// button-based actions can sit inline next to plain text links.
export const captionLinkSx: SxProps<Theme> = (theme) => ({
  ...theme.typography.caption,
  color: theme.palette.text.secondary,
  textTransform: "none",
  padding: 0,
  minWidth: 0,
  verticalAlign: "baseline",
  "&:hover": {
    backgroundColor: "transparent",
    textDecoration: "underline",
  },
});
