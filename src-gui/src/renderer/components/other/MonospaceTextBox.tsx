import { Box, Typography } from "@mui/material";

type Props = {
  children: React.ReactNode;
  light?: boolean;
  centered?: boolean;
  actions?: React.ReactNode;
};

export default function MonospaceTextBox({
  children,
  light = false,
  centered = false,
  actions,
}: Props) {
  return (
    <Box
      sx={(theme) => ({
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        backgroundColor: light ? "transparent" : theme.palette.grey[900],
        borderRadius: 2,
        border: light ? `1px solid ${theme.palette.grey[800]}` : "none",
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
          ...(centered ? { textAlign: "center" } : {}),
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
