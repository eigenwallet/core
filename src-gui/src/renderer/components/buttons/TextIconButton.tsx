import {
  Box,
  IconButton as MuiIconButton,
  IconButtonProps,
  Typography,
  SxProps,
} from "@mui/material";

export default function TextIconButton({
  children,
  label,
  isMainActionButton = false,
  ...props
}: IconButtonProps & { label: string, isMainActionButton?: boolean }) {
  const iconButtonStyles: SxProps = {
    width: "100%",
    aspectRatio: "1/1",
    bgcolor: "grey.900",
    border: "1px solid",
    borderColor: "grey.800",
  }

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 1,
        flex: "1 1 0",
      }}
    >
      <MuiIconButton sx={isMainActionButton ? iconButtonStyles : {}} {...props}>{children}</MuiIconButton>
      <Typography variant="body2">{label}</Typography>
    </Box>
  );
}
