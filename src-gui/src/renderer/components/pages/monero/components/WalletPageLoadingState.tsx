import { Box, Card, Chip, Skeleton, Typography } from "@mui/material";
import StateIndicator from "./StateIndicator";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";

const DUMMY_ADDRESS =
  "888tNkZrPN6JsEgekjMnABU4TBzc2Dt29EPAvkRxbANsAnjyPbb3iQ1YBRk1UXcdRsiKc9dhwMVgN5S9cQUiyoogDavup3H";

export default function WalletPageLoadingState() {
  return (
    <Box
      sx={{
        maxWidth: 800,
        mx: "auto",
        display: "flex",
        flexDirection: "column",
        gap: 2,
        pb: 2,
      }}
    >
      <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
        {/* Balance */}
        <Box
          sx={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "flex-start",
            mb: 1,
          }}
        >
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: 0.5,
            }}
          >
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
              Available Funds
            </Typography>
            <Typography variant="h4">
              <Skeleton variant="text" width="80%" />
            </Typography>
            <Typography variant="body2" color="text.secondary">
              <Skeleton variant="text" width="40%" />
            </Typography>
          </Box>

          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
            }}
          >
            <Box
              sx={{
                display: "flex",
                flexDirection: "row",
                alignItems: "center",
                gap: 1,
              }}
            >
              <StateIndicator color="primary" pulsating={true} />
            </Box>
          </Box>
        </Box>
      </Card>

      <Skeleton variant="rounded" width="100%">
        <ActionableMonospaceTextBox content={DUMMY_ADDRESS} />
      </Skeleton>

      <Box sx={{ display: "flex", flexDirection: "row", gap: 2, mb: 2 }}>
        {Array.from({ length: 2 }).map((_) => (
          <Skeleton variant="rounded" sx={{ borderRadius: "100px" }}>
            <Chip label="Loading..." variant="button" />
          </Skeleton>
        ))}
      </Box>

      <Typography variant="h5">Transaction History</Typography>
      <Skeleton variant="rounded" width="100%" height={40} />
    </Box>
  );
}
