import { Typography, Box, Paper, Divider, Pagination } from "@mui/material";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import MakerOfferItem from "./MakerOfferItem";
import { usePendingSelectMakerApproval } from "store/hooks";
import MakerDiscoveryStatus from "./MakerDiscoveryStatus";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SatsAmount } from "renderer/components/other/Units";
import { useState, useMemo, useCallback } from "react";
import { sortApprovalsAndKnownQuotes } from "utils/sortUtils";

export default function DepositAndChooseOfferPage({
  deposit_address,
  max_giveable,
  known_quotes,
}: TauriSwapProgressEventContent<"WaitingForBtcDeposit">) {
  const pendingSelectMakerApprovals = usePendingSelectMakerApproval();
  const [currentPage, setCurrentPage] = useState(1);
  const offersPerPage = 3;

  // Memoize sorting to avoid recalculating on every render
  const makerOffers = useMemo(
    () => sortApprovalsAndKnownQuotes(pendingSelectMakerApprovals, known_quotes),
    [pendingSelectMakerApprovals, known_quotes],
  );

  // Memoize pagination calculations
  const { totalPages, startIndex, endIndex, paginatedOffers } = useMemo(() => {
    const total = Math.ceil(makerOffers.length / offersPerPage);
    const start = (currentPage - 1) * offersPerPage;
    const end = start + offersPerPage;
    const paginated = makerOffers.slice(start, end);
    return {
      totalPages: total,
      startIndex: start,
      endIndex: end,
      paginatedOffers: paginated,
    };
  }, [makerOffers, currentPage, offersPerPage]);

  const handlePageChange = useCallback(
    (event: React.ChangeEvent<unknown>, value: number) => {
      setCurrentPage(value);
    },
    [],
  );

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 3,
      }}
    >
      <Box
        sx={{
          display: "flex",
          flexDirection: { xs: "column", md: "row" },
          gap: 2,
        }}
      >
        <Box sx={{ flexGrow: 1, flexShrink: 0, minWidth: "12em" }}>
          <Typography variant="body1">Bitcoin Balance</Typography>
          <Typography variant="h5">
            <SatsAmount amount={max_giveable} />
          </Typography>
        </Box>

        <Divider
          orientation="vertical"
          flexItem
          sx={{
            marginX: { xs: 0, md: 1 },
            marginY: { xs: 1, md: 0 },
            display: { xs: "none", md: "block" },
          }}
        />
        <Divider
          orientation="horizontal"
          flexItem
          sx={{
            marginX: { xs: 0, md: 1 },
            marginY: { xs: 1, md: 0 },
            display: { xs: "block", md: "none" },
          }}
        />

        <Box
          sx={{
            flexShrink: 1,
            display: "flex",
            flexDirection: "column",
            gap: 1,
          }}
        >
          <Typography variant="body1">Deposit</Typography>
          <Typography variant="body2" color="text.secondary">
            Send Bitcoin to your internal wallet to swap your desired amount of
            Monero
          </Typography>
          <ActionableMonospaceTextBox content={deposit_address} />
        </Box>
      </Box>

      {/* Available Makers Section */}
      <Box>
        {/* Maker Discovery Status */}
        <MakerDiscoveryStatus />

        {/* Real Maker Offers */}
        <Box>
          {makerOffers.length > 0 && (
            <>
              <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                {paginatedOffers.map((quote, index) => {
                  return (
                    <MakerOfferItem
                      key={startIndex + index}
                      quoteWithAddress={quote.quote_with_address}
                      requestId={quote.approval?.request_id}
                    />
                  );
                })}
              </Box>

              {totalPages > 1 && (
                <Box sx={{ display: "flex", justifyContent: "center", mt: 2 }}>
                  <Pagination
                    count={totalPages}
                    page={currentPage}
                    onChange={handlePageChange}
                    color="primary"
                  />
                </Box>
              )}
            </>
          )}

          {/* TODO: Differentiate between no makers found and still loading */}
          {makerOffers.length === 0 && (
            <Paper variant="outlined" sx={{ p: 3, textAlign: "center" }}>
              <Typography variant="body1" color="textSecondary">
                Searching for available makers...
              </Typography>
              <Typography variant="body2" color="textSecondary" sx={{ mt: 1 }}>
                Please wait while we find the best offers for your swap.
              </Typography>
            </Paper>
          )}
        </Box>
      </Box>
    </Box>
  );
}
