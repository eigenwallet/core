import { Box, Typography } from "@mui/material";

type Props = {
  children: React.ReactNode;
  light?: boolean;
  actions?: React.ReactNode;
};

export default function MonospaceTextBox({
  children,
  light = false,
  actions,
}: Props) {
  return (
    <Box
      sx={(theme) => ({
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        backgroundColor: theme.palette.action.hover,
        borderRadius: theme.shape.borderRadius,
        border: "none",
        padding: theme.spacing(1),
        gap: 1,
      })}
    >
      <Typography
        component="span"
        variant="overline"
        sx={{
          wordBreak: "break-word",
          whiteSpace: "pre-wrap",
          fontFamily: "monospace",
          lineHeight: 1.5,
          flex: 1,
        }}
      >
        {children}
      </Typography>
      {actions && (
        <Box sx={{ display: "flex", gap: 0.5, flexShrink: 0 }}>{actions}</Box>
      )}
    </Box>
  );
}
