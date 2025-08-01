import {
  Box,
  IconButton as MuiIconButton,
  IconButtonProps,
  Typography,
} from "@mui/material";

export default function TextIconButton({
  children,
  label,
  ...props
}: IconButtonProps & { label: string }) {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 1,
      }}
    >
      <MuiIconButton {...props}>{children}</MuiIconButton>
      <Typography variant="body2">{label}</Typography>
    </Box>
  );
}
