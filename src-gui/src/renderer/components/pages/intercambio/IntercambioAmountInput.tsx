import { Box, Card, Typography } from "@mui/material";
import NumberInput from "renderer/components/inputs/NumberInput";
import { useTheme } from "@mui/material/styles";

interface IntercambioAmountInputProps {
  btcAmount: string;
  onBtcAmountChange: (amount: string) => void;
  estimatedXmr?: number;
  disabled?: boolean;
}

export default function IntercambioAmountInput({
  btcAmount,
  onBtcAmountChange,
  estimatedXmr,
  disabled = false,
}: IntercambioAmountInputProps) {
  const theme = useTheme();

  return (
    <Card
      sx={{
        p: 3,
        background: theme.palette.background.paper,
        borderRadius: 2,
      }}
    >
      <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <NumberInput
            value={btcAmount}
            onChange={onBtcAmountChange}
            placeholder="0.00"
            fontSize="2rem"
            fontWeight={600}
            step={0.0001}
            largeStep={0.001}
          />
          <Typography
            variant="h5"
            sx={{
              fontWeight: 600,
              color: theme.palette.text.secondary,
              ml: 2,
            }}
          >
            BTC
          </Typography>
        </Box>

        {estimatedXmr !== undefined && (
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              opacity: 0.6,
            }}
          >
            <Typography variant="body1">
              ≈ {estimatedXmr.toFixed(4)}
            </Typography>
            <Typography variant="body1">XMR</Typography>
          </Box>
        )}
      </Box>
    </Card>
  );
}

