import {
  Box,
  Chip,
  IconButton,
  Menu,
  MenuItem,
  Typography,
  Dialog,
  DialogActions,
  Button,
  TableContainer,
  Table,
  TableHead,
  TableBody,
  TableRow,
  TableCell,
} from "@mui/material";
import {
  TransactionDirection,
  TransactionInfo,
  Amount,
} from "models/tauriModel";
import { PiconeroAmountArgs } from "renderer/components/other/Units";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";

// https://stackoverflow.com/a/22015930/2851815
function zip<A, B>(a: A[], b: B[]) {
  return Array(Math.max(b.length, a.length))
    .fill(undefined)
    .map((_, i) => [a[i], b[i]]);
}

export default function TransactionDetailsDialog({
  open,
  onClose,
  transaction,
  UnitAmount,
}: {
  open: boolean;
  onClose: () => void;
  transaction: TransactionInfo;
  UnitAmount: React.FC<PiconeroAmountArgs>;
}) {
  const rowKey = (input: [string, number], output: [string, number]) =>
    `${input && input[0]}${output && output[0]}`;
  const rowPair = (split: [string, number]) => {
    if (!split) return <TableCell colSpan={2} />;

    const [id, amount] = split;
    return (
      <>
        <TableCell>
          <ActionableMonospaceTextBox
            displayCopyIcon={false}
            enableQrCode={false}
            content={id}
          />
        </TableCell>
        <TableCell>
          <UnitAmount
            amount={amount}
            labelStyles={{ fontSize: 14, ml: -0.3 }}
            disableTooltip
          />
        </TableCell>
      </>
    );
  };
  const rows =
    transaction.splits &&
    zip(transaction.splits.inputs, transaction.splits.outputs).map(
      ([input, output]) => {
        return (
          <TableRow key={rowKey(input, output)}>
            {rowPair(input)}
            {rowPair(output)}
          </TableRow>
        );
      },
    );

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <ActionableMonospaceTextBox
        displayCopyIcon={false}
        enableQrCode={false}
        content={transaction.tx_hash}
        centered
      />

      <TableContainer>
        <Table>
          <TableHead>
            {transaction.splits && (
              <TableRow>
                <TableCell>Input</TableCell>
                <TableCell>Amount</TableCell>
                <TableCell>Output</TableCell>
                <TableCell>Amount</TableCell>
              </TableRow>
            )}
          </TableHead>
          <TableBody>
            {rows}
            <TableRow>
              <TableCell component="th">Fee</TableCell>
              <TableCell colSpan={4}>
                <UnitAmount
                  amount={transaction.fee}
                  labelStyles={{ fontSize: 14, ml: -0.3 }}
                  disableTooltip
                />
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </TableContainer>

      <DialogActions>
        <Button onClick={onClose} color="primary" variant="text">
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}
