import {
  Box,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
  Skeleton,
} from "@mui/material";
import { useSwapInfosSortedByDate, useAreSwapInfosLoaded } from "store/hooks";
import HistoryRow from "./HistoryRow";

function SkeletonRow({ animate = true }: { animate?: boolean }) {
  const animation = animate ? "pulse" : false;
  return (
    <TableRow>
      <TableCell>
        <Skeleton
          animation={animation}
          variant="circular"
          width={24}
          height={24}
        />
      </TableCell>
      <TableCell>
        <Skeleton animation={animation} variant="text" width="80%" />
      </TableCell>
      <TableCell>
        <Skeleton animation={animation} variant="text" width="60%" />
      </TableCell>
      <TableCell>
        <Skeleton
          animation={animation}
          variant="rectangular"
          width={80}
          height={24}
        />
      </TableCell>
      <TableCell>
        <Skeleton
          animation={animation}
          variant="circular"
          width={24}
          height={24}
        />
      </TableCell>
    </TableRow>
  );
}

function SkeletonRows({ animate = true }: { animate?: boolean }) {
  return (
    <>
      {Array.from({ length: 3 }).map((_, index) => (
        <SkeletonRow key={index} animate={animate} />
      ))}
    </>
  );
}

function EmptyState() {
  return (
    <>
      <TableRow>
        <TableCell colSpan={5} sx={{ textAlign: "center", py: 4 }}>
          <Typography variant="h6" color="text.secondary" gutterBottom>
            Nothing to see here
          </Typography>
          <Typography variant="body2" color="text.secondary">
            You haven't made any swaps yet
          </Typography>
        </TableCell>
      </TableRow>
      <SkeletonRows animate={false} />
    </>
  );
}

export default function HistoryTable() {
  const swapSortedByDate = useSwapInfosSortedByDate();
  const areSwapInfosLoaded = useAreSwapInfosLoaded();

  function renderContent() {
    if (!areSwapInfosLoaded) {
      return <SkeletonRows />;
    }

    if (swapSortedByDate.length === 0) {
      return <EmptyState />;
    }

    return swapSortedByDate.map((swap) => (
      <HistoryRow {...swap} key={swap.swap_id} />
    ));
  }

  return (
    <Box
      sx={{
        paddingTop: 1,
        paddingBottom: 1,
      }}
    >
      <TableContainer component={Paper}>
        <Table>
          {swapSortedByDate.length > 0 && (
            <TableHead>
              <TableRow>
                <TableCell />
                <TableCell>ID</TableCell>
                <TableCell>Amount</TableCell>
                <TableCell>State</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
          )}
          <TableBody>{renderContent()}</TableBody>
        </Table>
      </TableContainer>
    </Box>
  );
}
