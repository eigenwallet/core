import {
  Box,
  Link,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Typography,
} from "@mui/material";
import { GetSwapInfoResponse } from "models/tauriModel";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import {
  MoneroBitcoinExchangeRateFromAmounts,
  PiconeroAmount,
  SatsAmount,
} from "renderer/components/other/Units";
import { isTestnet } from "store/config";
import { getBitcoinTxExplorerUrl } from "utils/conversionUtils";
import SwapLogFileOpenButton from "./SwapLogFileOpenButton";
import ExportLogsButton from "./ExportLogsButton";

const expandedContainerSx = {
  display: "grid",
  padding: 1,
  gap: 1,
};

const makerAddressContainerSx = {
  display: "flex",
  flexDirection: "column",
  gap: 1,
};

const poolContainerSx = {
  display: "flex",
  flexDirection: "column",
  gap: 1,
};

const poolItemSx = {
  display: "flex",
  flexDirection: "column",
  gap: 0.5,
  padding: 1,
  border: 1,
  borderColor: "divider",
  borderRadius: 1,
  backgroundColor: (theme: any) => theme.palette.action.hover,
};

const poolLabelSx = (theme: any) => ({
  fontWeight: 600,
  color: theme.palette.text.primary,
});

const poolAddressSx = {
  fontFamily: "monospace",
  color: (theme: any) => theme.palette.text.secondary,
  wordBreak: "break-all",
};

const actionsContainerSx = {
  display: "flex",
  flexDirection: "row",
  gap: 1,
};

export default function HistoryRowExpanded({
  swap,
}: {
  swap: GetSwapInfoResponse;
}) {
  return (
    <Box sx={expandedContainerSx}>
      <TableContainer>
        <Table>
          <TableBody>
            <TableRow>
              <TableCell>Started on</TableCell>
              <TableCell>{swap.start_date}</TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Swap ID</TableCell>
              <TableCell>{swap.swap_id}</TableCell>
            </TableRow>
            <TableRow>
              <TableCell>State Name</TableCell>
              <TableCell>{swap.state_name}</TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Monero Amount</TableCell>
              <TableCell>
                <PiconeroAmount amount={swap.xmr_amount} />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Bitcoin Amount</TableCell>
              <TableCell>
                <SatsAmount amount={swap.btc_amount} />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Exchange Rate</TableCell>
              <TableCell>
                <MoneroBitcoinExchangeRateFromAmounts
                  satsAmount={swap.btc_amount}
                  piconeroAmount={swap.xmr_amount}
                />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Bitcoin Network Fees</TableCell>
              <TableCell>
                <SatsAmount amount={swap.tx_lock_fee} />
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Maker Address</TableCell>
              <TableCell>
                <Box sx={makerAddressContainerSx}>
                  {swap.seller.addresses.map((addr) => (
                    <ActionableMonospaceTextBox
                      key={addr}
                      content={addr}
                      displayCopyIcon={true}
                      enableQrCode={false}
                    />
                  ))}
                </Box>
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Bitcoin lock transaction</TableCell>
              <TableCell>
                <Link
                  href={getBitcoinTxExplorerUrl(swap.tx_lock_id, isTestnet())}
                  target="_blank"
                >
                  <MonospaceTextBox>{swap.tx_lock_id}</MonospaceTextBox>
                </Link>
              </TableCell>
            </TableRow>
            <TableRow>
              <TableCell>Monero receive pool</TableCell>
              <TableCell>
                <Box sx={poolContainerSx}>
                  {swap.monero_receive_pool.map((pool, index) => (
                    <Box key={index} sx={poolItemSx}>
                      <Typography variant="body2" sx={poolLabelSx}>
                        {pool.label} ({pool.percentage * 100}%)
                      </Typography>
                      <Typography variant="caption" sx={poolAddressSx}>
                        {pool.address}
                      </Typography>
                    </Box>
                  ))}
                </Box>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </TableContainer>
      <Box sx={actionsContainerSx}>
        <SwapLogFileOpenButton
          swapId={swap.swap_id}
          variant="outlined"
          size="small"
        />
        <ExportLogsButton
          swap_id={swap.swap_id}
          variant="outlined"
          size="small"
        />
      </Box>
    </Box>
  );
}
