import {
  Typography,
  Box,
  Paper,
  Divider,
  Pagination,
  Menu,
  MenuItem,
  ListItemIcon,
  ListItemText,
  Tooltip,
} from "@mui/material";
import SortIcon from "@mui/icons-material/Sort";
import CheckIcon from "@mui/icons-material/Check";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import MakerOfferItem from "./MakerOfferItem";
import { usePendingSelectMakerApproval } from "store/hooks";
import MakerDiscoveryStatus from "./MakerDiscoveryStatus";
import { TauriSwapProgressEventContent } from "models/tauriModelExt";
import { SatsAmount } from "renderer/components/other/Units";
import { useEffect, useState } from "react";
import { sortApprovalsAndKnownQuotes, OfferSortMode } from "utils/sortUtils";

const SORT_OPTIONS: { value: OfferSortMode; label: string }[] = [
  { value: "large", label: "Large swaps" },
  { value: "small", label: "Small swaps" },
  { value: "cheapest", label: "Cheapest" },
];

export default function DepositAndChooseOfferPage({
  deposit_address,
  max_giveable,
  known_quotes,
}: TauriSwapProgressEventContent<"WaitingForBtcDeposit">) {
  const pendingSelectMakerApprovals = usePendingSelectMakerApproval();
  const [currentPage, setCurrentPage] = useState(1);
  const [sortMode, setSortMode] = useState<OfferSortMode>("large");
  const [sortAnchorEl, setSortAnchorEl] = useState<null | HTMLElement>(null);
  const offersPerPage = 3;

  const makerOffers = sortApprovalsAndKnownQuotes(
    pendingSelectMakerApprovals,
    known_quotes,
    sortMode,
  );

  const currentSortLabel =
    SORT_OPTIONS.find((o) => o.value === sortMode)?.label ?? "";

  // Pagination calculations
  const totalPages = Math.max(1, Math.ceil(makerOffers.length / offersPerPage));

  useEffect(() => {
    if (currentPage > totalPages) {
      setCurrentPage(totalPages);
    }
  }, [currentPage, totalPages]);

  const clampedPage = Math.min(currentPage, totalPages);
  const startIndex = (clampedPage - 1) * offersPerPage;
  const endIndex = startIndex + offersPerPage;
  const paginatedOffers = makerOffers.slice(startIndex, endIndex);

  const handlePageChange = (
    event: React.ChangeEvent<unknown>,
    value: number,
  ) => {
    setCurrentPage(value);
  };

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
              <Box
                sx={{
                  display: "flex",
                  justifyContent: "flex-end",
                  alignItems: "center",
                  mb: 0.5,
                }}
              >
                <Tooltip title={`Sort: ${currentSortLabel}`}>
                  <Box
                    component="button"
                    type="button"
                    onClick={(e) =>
                      setSortAnchorEl(e.currentTarget as HTMLElement)
                    }
                    sx={{
                      display: "flex",
                      alignItems: "center",
                      gap: 0.5,
                      px: 0.75,
                      py: 0.25,
                      border: "none",
                      background: "transparent",
                      color: "inherit",
                      cursor: "pointer",
                      borderRadius: 1,
                      opacity: 0.6,
                      "&:hover": {
                        opacity: 1,
                        bgcolor: "action.hover",
                      },
                    }}
                  >
                    <Typography variant="caption" color="text.secondary">
                      Sorting
                    </Typography>
                    <SortIcon fontSize="small" />
                  </Box>
                </Tooltip>
                <Menu
                  anchorEl={sortAnchorEl}
                  open={Boolean(sortAnchorEl)}
                  onClose={() => setSortAnchorEl(null)}
                  anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
                  transformOrigin={{ vertical: "top", horizontal: "right" }}
                >
                  {SORT_OPTIONS.map((option) => (
                    <MenuItem
                      key={option.value}
                      selected={option.value === sortMode}
                      onClick={() => {
                        setSortMode(option.value);
                        setCurrentPage(1);
                        setSortAnchorEl(null);
                      }}
                      dense
                    >
                      <ListItemIcon sx={{ minWidth: 28 }}>
                        {option.value === sortMode && (
                          <CheckIcon fontSize="small" />
                        )}
                      </ListItemIcon>
                      <ListItemText>{option.label}</ListItemText>
                    </MenuItem>
                  ))}
                </Menu>
              </Box>
              <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
                {paginatedOffers.map((quote) => {
                  return (
                    <MakerOfferItem
                      key={quote.quote_with_address.peer_id}
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
                    page={clampedPage}
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
