import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TableFooter,
  TextField,
  Typography,
} from "@mui/material";
import AddOutlinedIcon from "@mui/icons-material/AddOutlined";
import { useSnackbar } from "notistack";
import type { SubaddressSummary } from "models/tauriModel";
import { useEffect, useMemo, useRef, useState } from "react";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { PiconeroAmount } from "renderer/components/other/Units";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import {
  createMoneroSubaddress,
  setMoneroSubaddressLabel,
  updateMoneroSubaddresses,
} from "renderer/rpc";
import { useMoneroSubaddresses } from "store/hooks";

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function SubaddressesModal({ open, onClose }: Props) {
  const subaddresses = useMoneroSubaddresses();
  const [isCreating, setIsCreating] = useState(false);
  const [labelEdits, setLabelEdits] = useState<Record<number, string>>({});
  const { enqueueSnackbar } = useSnackbar();

  // Save all pending edits when dialog closes
  const handleClose = async () => {
    // Save all pending edits
    const pendingEdits = Object.entries(labelEdits);
    if (pendingEdits.length > 0) {
      await Promise.all(
        pendingEdits.map(async ([addressIndex, value]) => {
          const index = Number(addressIndex);
          const subaddress = subaddressesByIndex[index];
          if (!subaddress) return;

          const trimmedValue = value.trim();
          const currentValue = (subaddress.label ?? "").trim();

          // Only save if the value has changed
          if (trimmedValue === currentValue) return;

          try {
            await setMoneroSubaddressLabel(
              subaddress.account_index,
              subaddress.address_index,
              trimmedValue,
            );
          } catch (err) {
            const message =
              err instanceof Error
                ? `Failed to save label: ${err.message}`
                : "Failed to save label";
            enqueueSnackbar(message, { variant: "error" });
          }
        }),
      );

      // Refresh subaddresses after all saves
      if (pendingEdits.length > 0) {
        try {
          await updateMoneroSubaddresses();
        } catch (err) {
          // Ignore errors on refresh
        }
      }
    }

    onClose();
  };

  const subaddressesByIndex = useMemo(
    () =>
      subaddresses.reduce<Record<number, SubaddressSummary>>((acc, curr) => {
        acc[curr.address_index] = curr;
        return acc;
      }, {}),
    [subaddresses],
  );

  const createAddress = async () => {
    setIsCreating(true);
    try {
      await createMoneroSubaddress("");
      await updateMoneroSubaddresses();
    } catch (err) {
      const message =
        err instanceof Error
          ? `Could not create subaddress: ${err.message}`
          : "Could not create subaddress";
      enqueueSnackbar(message, { variant: "error" });
      throw err;
    } finally {
      setIsCreating(false);
    }
  };

  const handleLabelChange = (addressIndex: number, value: string) => {
    setLabelEdits((prev) => ({ ...prev, [addressIndex]: value }));
  };

  return (
    <Dialog open={open} onClose={handleClose} maxWidth="md" fullWidth>
      <DialogTitle>Subaddresses</DialogTitle>
      <DialogContent dividers sx={{ padding: 0 }}>
        <Table size="small" stickyHeader>
          <TableHead>
            <TableRow>
              <TableCell
                align="center"
                width="10%"
                sx={{ whiteSpace: "nowrap" }}
              >
                Index
              </TableCell>
              <TableCell align="center" sx={{ whiteSpace: "nowrap" }}>
                Address
              </TableCell>
              <TableCell
                align="center"
                width="25%"
                sx={{ whiteSpace: "nowrap" }}
              >
                Label
              </TableCell>
              <TableCell
                align="center"
                width="15%"
                sx={{ whiteSpace: "nowrap" }}
              >
                Unlocked balance
              </TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {subaddresses.length === 0 ? (
              <TableRow>
                <TableCell colSpan={4}>
                  <Typography color="text.secondary">
                    No subaddresses yet. Add one to start labeling.
                  </Typography>
                </TableCell>
              </TableRow>
            ) : (
              subaddresses.map((s) => {
                const labelDraft =
                  labelEdits[s.address_index] ??
                  subaddressesByIndex[s.address_index]?.label ??
                  "";

                return (
                  <TableRow key={s.address_index} hover>
                    <TableCell
                      align="center"
                      sx={{ verticalAlign: "middle !important" }}
                    >
                      <Typography variant="body2" color="text.secondary">
                        #{s.address_index}
                      </Typography>
                    </TableCell>
                    <TableCell
                      align="center"
                      sx={{
                        verticalAlign: "middle !important",
                        maxWidth: "20rem",
                      }}
                    >
                      <ActionableMonospaceTextBox
                        content={s.address}
                        truncate
                      />
                    </TableCell>
                    <TableCell
                      align="center"
                      sx={{ verticalAlign: "middle !important" }}
                    >
                      <TextField
                        size="small"
                        fullWidth
                        label="Label"
                        value={labelDraft}
                        onChange={(e) =>
                          handleLabelChange(s.address_index, e.target.value)
                        }
                      />
                    </TableCell>

                    <TableCell
                      align="center"
                      sx={{ verticalAlign: "middle !important" }}
                    >
                      <Typography variant="body2" color="text.secondary">
                        <PiconeroAmount amount={s.unlocked_balance} />
                      </Typography>
                    </TableCell>
                  </TableRow>
                );
              })
            )}
          </TableBody>
          <TableFooter>
            <TableRow>
              <TableCell colSpan={4}>
                <Box
                  sx={{
                    display: "flex",
                    gap: 1,
                    alignItems: "center",
                    justifyContent: "flex-end",
                    flexWrap: "wrap",
                  }}
                >
                  <PromiseInvokeButton
                    startIcon={<AddOutlinedIcon />}
                    variant="contained"
                    disabled={isCreating}
                    onInvoke={createAddress}
                    displayErrorSnackbar={false}
                  >
                    Add subaddress
                  </PromiseInvokeButton>
                </Box>
              </TableCell>
            </TableRow>
          </TableFooter>
        </Table>
      </DialogContent>
      <DialogActions>
        <Button onClick={handleClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}
