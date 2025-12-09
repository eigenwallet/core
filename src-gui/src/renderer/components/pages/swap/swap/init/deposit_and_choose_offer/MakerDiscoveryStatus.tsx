import {
  Box,
  Typography,
  LinearProgress,
  Paper,
  IconButton,
  Dialog,
  DialogTitle,
  DialogContent,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TablePagination,
  Chip,
  CircularProgress,
  Stack,
  Tooltip,
} from "@mui/material";
import {
  Info as InfoIcon,
  Close as CloseIcon,
  Refresh as RefreshIcon,
} from "@mui/icons-material";
import { useEffect, useState, useMemo } from "react";
import { useAppSelector } from "store/hooks";
import { QuoteStatus, ConnectionStatus } from "models/tauriModel";
import { selectPeers } from "store/selectors";
import TorIcon from "renderer/components/icons/TorIcon";
import TruncatedText from "renderer/components/other/TruncatedText";
import ClickToCopy from "renderer/components/other/ClickToCopy";
import Jdenticon from "renderer/components/other/Jdenticon";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { refreshP2P } from "renderer/rpc";

type Peer = ReturnType<typeof selectPeers>[number];

export default function MakerDiscoveryStatus() {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [everConnectedPeers, setEverConnectedPeers] = useState<Set<string>>(
    new Set(),
  );
  const peers = useAppSelector(selectPeers);

  const connectedPeerIds = peers
    .filter((p) => p.connection === ConnectionStatus.Connected)
    .map((p) => p.peer_id);

  // Track peers that have ever been connected
  useEffect(() => {
    if (connectedPeerIds.length > 0) {
      setEverConnectedPeers((prev) => {
        const updated = new Set(prev);
        connectedPeerIds.forEach((id) => updated.add(id));
        return updated;
      });
    }
  }, [peers]);

  const quotesInflight = peers.filter(
    (p) => p.quote === QuoteStatus.Inflight,
  ).length;
  const dialsInflight = peers.filter(
    (p) => p.connection === ConnectionStatus.Dialing,
  ).length;

  const isActive = quotesInflight > 0 || dialsInflight > 0;

  return (
    <>
      <Tooltip title="Click to view details">
        <Paper
          variant="outlined"
          onClick={() => setDialogOpen(true)}
          sx={{
            width: "100%",
            mb: 2,
            p: 2,
            borderColor: isActive ? "success.main" : "divider",
            opacity: isActive ? 1 : 0.6,
            cursor: "pointer",
            transition: "background-color 0.2s",
            "&:hover": {
              bgcolor: "action.hover",
            },
          }}
        >
          <Stack gap={1.5}>
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
            >
              <Typography
                variant="body2"
                sx={{
                  fontWeight: "medium",
                  color: isActive ? "info.main" : "text.disabled",
                }}
              >
                {isActive
                  ? quotesInflight > 0
                    ? "Getting offers..."
                    : "Dialing peers..."
                  : "Waiting a few seconds..."}
              </Typography>

              <Stack direction="row" alignItems="center" gap={2}>
                <Stack direction="row" gap={2}>
                  <Typography
                    variant="caption"
                    sx={{
                      color: isActive ? "success.main" : "text.disabled",
                      fontWeight: "medium",
                    }}
                  >
                    Connected to {connectedPeerIds.length} peers
                  </Typography>
                </Stack>
                <InfoIcon
                  fontSize="small"
                  sx={{ opacity: 0.7, color: "action.active" }}
                />
              </Stack>
            </Stack>
            <LinearProgress
              variant={isActive ? "indeterminate" : "determinate"}
              value={0}
              sx={{
                width: "100%",
                height: 8,
                borderRadius: 4,
                opacity: isActive ? 1 : 0.4,
              }}
            />
          </Stack>
        </Paper>
      </Tooltip>
      <PeerDetailsDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        peers={peers}
        everConnectedPeers={everConnectedPeers}
      />
    </>
  );
}

function QuoteStatusChip({ status }: { status: QuoteStatus | null }) {
  switch (status) {
    case QuoteStatus.Received:
      return <Chip label="Got quote" color="success" size="small" />;
    case QuoteStatus.Inflight:
      return (
        <Chip
          label="Requesting"
          color="info"
          size="small"
          icon={<CircularProgress size={12} color="inherit" />}
        />
      );
    case QuoteStatus.Failed:
      return <Chip label="Failed" color="error" size="small" />;
    case QuoteStatus.NotSupported:
      return <Chip label="No offers" color="warning" size="small" />;
    case QuoteStatus.Nothing:
    case null:
      return <Chip label="--" size="small" />;
    default:
      return null;
  }
}

function ConnectionStatusChip({ status }: { status: ConnectionStatus | null }) {
  switch (status) {
    case ConnectionStatus.Connected:
      return <Chip label="Connected" color="success" size="small" />;
    case ConnectionStatus.Disconnected:
    case null:
      return <Chip label="Disconnected" color="default" size="small" />;
    case ConnectionStatus.Dialing:
      return (
        <Chip
          label="Dialing"
          color="info"
          size="small"
          icon={<CircularProgress size={12} color="inherit" />}
        />
      );
    default:
      return null;
  }
}

/**
 * Sorts peers based on connection history, quote status (optional), and peer ID.
 */
function sortPeers(
  peers: Peer[],
  everConnectedPeers: Set<string>,
  checkQuoteStatus: boolean,
): Peer[] {
  return [...peers].sort((a, b) => {
    // 1. Put peers that have never connected at the bottom
    const aEverConnected = everConnectedPeers.has(a.peer_id);
    const bEverConnected = everConnectedPeers.has(b.peer_id);

    if (aEverConnected !== bEverConnected) {
      return aEverConnected ? -1 : 1;
    }

    // 2. Put NotSupported quotes at the bottom
    if (checkQuoteStatus) {
      const aNotSupported = a.quote === QuoteStatus.NotSupported;
      const bNotSupported = b.quote === QuoteStatus.NotSupported;

      if (aNotSupported !== bNotSupported) {
        return aNotSupported ? 1 : -1;
      }
    }

    // 3. Sort alphabetically by peer_id
    return a.peer_id.localeCompare(b.peer_id);
  });
}

interface PeerTableProps {
  peers: Peer[];
  page: number;
  rowsPerPage: number;
}

function PeerTable({ peers, page, rowsPerPage }: PeerTableProps) {
  const paginatedPeers = useMemo(() => {
    const startIndex = page * rowsPerPage;
    return peers.slice(startIndex, startIndex + rowsPerPage);
  }, [peers, page, rowsPerPage]);

  const emptyRows = rowsPerPage - paginatedPeers.length;

  return (
    <TableContainer sx={{ maxHeight: "70vh" }}>
      <Table
        size="small"
        stickyHeader
        sx={{
          tableLayout: "fixed",
          "& .MuiTableRow-root": {
            height: "3rem",
          },
          "& .MuiTableCell-root": {
            verticalAlign: "middle",
          },
        }}
      >
        <TableHead>
          <TableRow>
            <TableCell sx={{ width: "25%" }}>Peer ID</TableCell>
            <TableCell sx={{ width: "35%" }}>Address</TableCell>
            <TableCell sx={{ width: "20%" }}>Connection</TableCell>
            <TableCell sx={{ width: "20%" }}>Quote</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {paginatedPeers.map((entry) => (
            <TableRow key={entry.peer_id}>
              <TableCell sx={{ textAlign: "center" }}>
                <ClickToCopy content={entry.peer_id}>
                  <Stack
                    direction="row"
                    alignItems="center"
                    justifyContent="center"
                    gap={1}
                  >
                    <Jdenticon value={entry.peer_id} size={24} />
                    <Typography
                      variant="body2"
                      sx={{ fontFamily: "monospace", fontSize: "0.75rem" }}
                    >
                      <TruncatedText limit={16} truncateMiddle>
                        {entry.peer_id}
                      </TruncatedText>
                    </Typography>
                  </Stack>
                </ClickToCopy>
              </TableCell>
              <TableCell>
                <ClickToCopy
                  content={entry.last_address ?? ""}
                  showTooltip={!!entry.last_address}
                >
                  <Stack direction="row" alignItems="center" gap={0.5}>
                    <Typography
                      variant="body2"
                      sx={{
                        fontFamily: "monospace",
                        fontSize: "0.7rem",
                        maxWidth: 200,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {entry.last_address ?? "--"}
                    </Typography>
                    {entry.last_address?.includes("/onion3/") && (
                      <TorIcon
                        sx={{ fontSize: "0.9rem", color: "text.secondary" }}
                      />
                    )}
                  </Stack>
                </ClickToCopy>
              </TableCell>
              <TableCell>
                <ConnectionStatusChip status={entry.connection} />
              </TableCell>
              <TableCell>
                <QuoteStatusChip status={entry.quote} />
              </TableCell>
            </TableRow>
          ))}
          {emptyRows > 0 &&
            Array.from({ length: emptyRows }).map((_, index) => (
              <TableRow key={`empty-${index}`}>
                <TableCell colSpan={4} />
              </TableRow>
            ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}

interface PeerDetailsDialogProps {
  open: boolean;
  onClose: () => void;
  peers: Peer[];
  everConnectedPeers: Set<string>;
}

function PeerDetailsDialog({
  open,
  onClose,
  peers,
  everConnectedPeers,
}: PeerDetailsDialogProps) {
  const [page, setPage] = useState(0);
  const rowsPerPage = 8;

  const sortedPeers = useMemo(() => {
    return sortPeers(peers, everConnectedPeers, true);
  }, [peers, everConnectedPeers]);

  const handleChangePage = (_event: unknown, newPage: number) => {
    setPage(newPage);
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>
        <Stack
          direction="row"
          justifyContent="space-between"
          alignItems="center"
        >
          Peers
          <Stack direction="row" alignItems="center" gap={1}>
            <PromiseInvokeButton
              isIconButton
              onInvoke={refreshP2P}
              tooltipTitle="Force a network refresh"
              size="small"
            >
              <RefreshIcon />
            </PromiseInvokeButton>
            <IconButton onClick={onClose} size="small">
              <CloseIcon />
            </IconButton>
          </Stack>
        </Stack>
      </DialogTitle>
      <DialogContent sx={{ p: 0 }}>
        {peers.length === 0 ? (
          <Box sx={{ p: 3 }}>
            <Typography color="text.secondary">
              No peers discovered yet.
            </Typography>
          </Box>
        ) : (
          <Stack>
            <PeerTable
              peers={sortedPeers}
              page={page}
              rowsPerPage={rowsPerPage}
            />
            <TablePagination
              component="div"
              count={sortedPeers.length}
              page={page}
              onPageChange={handleChangePage}
              rowsPerPage={rowsPerPage}
              rowsPerPageOptions={[]}
              sx={{
                borderTop: 1,
                borderColor: "divider",
              }}
            />
          </Stack>
        )}
      </DialogContent>
    </Dialog>
  );
}
