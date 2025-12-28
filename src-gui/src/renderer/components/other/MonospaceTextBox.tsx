import { Box, Typography } from "@mui/material";

type Props = {
  children: React.ReactNode;
  light?: boolean;
  actions?: React.ReactNode;
  truncate?: boolean;
};

export default function MonospaceTextBox({
  children,
  light = false,
  actions,
  truncate = false,
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
          wordBreak: truncate ? "normal" : "break-word",
          whiteSpace: truncate ? "nowrap" : "pre-wrap",
          overflow: truncate ? "hidden" : "visible",
          textOverflow: truncate ? "ellipsis" : "clip",
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
