import { Box, Typography, Card, LinearProgress } from "@mui/material";
import { PiconeroAmount } from "renderer/components/other/Units";
import { FiatPiconeroAmount } from "renderer/components/other/Units";
import StateIndicator from "renderer/components/pages/monero/components/StateIndicator";
import useMoneroSyncProgress from "utils/useMoneroSyncProgress";

interface WalletOverviewProps {
  balance?: {
    unlocked_balance: string;
    total_balance: string;
  };
}

// Component for displaying wallet address and balance
export default function WalletOverview({
  balance,
}: WalletOverviewProps) {

  const pendingBalance = parseFloat(balance.total_balance) - parseFloat(balance.unlocked_balance);

  const { isSyncing, loadingBarStyle, loadingBarPercentage, loadingBarBuffer, primaryProgressInformation, secondaryProgressInformation } = useMoneroSyncProgress();

  return (
    <Card sx={{ p: 2, position: "relative", borderRadius: 2 }} elevation={4}>
      {isSyncing && (
        <LinearProgress
          value={loadingBarPercentage}
          valueBuffer={loadingBarBuffer}
          variant={loadingBarStyle}
          sx={{
            position: "absolute",
            top: 0,
            left: 0,
            width: "100%",
          }}
        />
      )}

      {/* Balance */}
      <Box
        sx={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "stretch",
          mb: 1,
        }}
      >
        {/* Left side content */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "row",
            gap: 4,
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
              <PiconeroAmount
                amount={parseFloat(balance.unlocked_balance)}
                fixedPrecision={4}
                disableTooltip
              />
            </Typography>
            <Typography variant="body2" color="text.secondary">
              <FiatPiconeroAmount
                amount={parseFloat(balance.unlocked_balance)}
              />
            </Typography>
          </Box>
          {pendingBalance > 0 && (
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                gap: 0.5,
              }}
            >
              <Typography
                variant="body2"
                color="warning"
                sx={{
                  mb: 1,
                  animation: "pulse 2s infinite",
                }}
              >
                Pending
              </Typography>
              <Typography variant="h5">
                <PiconeroAmount amount={pendingBalance} fixedPrecision={4} />
              </Typography>
              <Typography variant="body2" color="text.secondary">
                <FiatPiconeroAmount amount={pendingBalance} />
              </Typography>
            </Box>
          )}
        </Box>

        {/* Right side - simple approach */}
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
            justifyContent: "space-between",
            minHeight: "100%",
          }}
        >
          <StateIndicator
            color={isSyncing ? "primary" : "success"}
            pulsating={isSyncing}
          />
          <Box sx={{ textAlign: "right" }}>
            {secondaryProgressInformation && (
              <>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ mt: 0.5, fontSize: "0.7rem", display: "block" }}
                >
                  {secondaryProgressInformation}
                </Typography>
              </>
            )}
            {primaryProgressInformation && (
              <Typography variant="body2" color="text.secondary">
                {primaryProgressInformation}
              </Typography>
            )}
          </Box>
        </Box>
      </Box>
    </Card>
  );
}
