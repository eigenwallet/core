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

const SAVE_DELAY_MS = 5000;
const SAVED_LABEL_DURATION_MS = 2500;

export default function SubaddressesModal({ open, onClose }: Props) {
  const subaddresses = useMoneroSubaddresses();
  const [isCreating, setIsCreating] = useState(false);
  const [labelEdits, setLabelEdits] = useState<Record<number, string>>({});
  const [labelErrors, setLabelErrors] = useState<Record<number, string>>({});
  const [savingLabels, setSavingLabels] = useState<Record<number, boolean>>({});
  const [recentlySaved, setRecentlySaved] = useState<Record<number, boolean>>(
    {},
  );
  const saveTimers = useRef<Record<number, ReturnType<typeof setTimeout>>>({});
  const savedLabelTimers = useRef<
    Record<number, ReturnType<typeof setTimeout>>
  >({});
  const { enqueueSnackbar } = useSnackbar();

  useEffect(() => {
    return () => {
      Object.values(saveTimers.current).forEach((timer) => {
        clearTimeout(timer);
      });
      Object.values(savedLabelTimers.current).forEach((timer) => {
        clearTimeout(timer);
      });
    };
  }, []);

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

  const persistLabel = async (s: SubaddressSummary, value: string) => {
    const trimmedValue = value.trim();
    const currentValue = (s.label ?? "").trim();

    if (saveTimers.current[s.address_index]) {
      clearTimeout(saveTimers.current[s.address_index]);
      delete saveTimers.current[s.address_index];
    }

    if (trimmedValue === currentValue) {
      setLabelEdits((prev) => {
        const next = { ...prev };
        delete next[s.address_index];
        return next;
      });
      return;
    }

    setSavingLabels((prev) => ({ ...prev, [s.address_index]: true }));
    let saveSucceeded = false;
    try {
      await setMoneroSubaddressLabel(
        s.account_index,
        s.address_index,
        trimmedValue,
      );

      await updateMoneroSubaddresses();

      setLabelEdits((prev) => {
        const next = { ...prev };
        delete next[s.address_index];
        return next;
      });

      setLabelErrors((prev) => {
        const next = { ...prev };
        delete next[s.address_index];
        return next;
      });

      setRecentlySaved((prev) => ({ ...prev, [s.address_index]: true }));
      saveSucceeded = true;
    } catch (err) {
      const message =
        err instanceof Error
          ? `Failed to save label: ${err.message}`
          : "Failed to save label";
      setLabelErrors((prev) => ({ ...prev, [s.address_index]: message }));
      enqueueSnackbar(message, { variant: "error" });
      return;
    } finally {
      setSavingLabels((prev) => {
        const next = { ...prev };
        delete next[s.address_index];
        return next;
      });
    }

    if (saveSucceeded) {
      if (savedLabelTimers.current[s.address_index]) {
        clearTimeout(savedLabelTimers.current[s.address_index]);
      }

      savedLabelTimers.current[s.address_index] = setTimeout(() => {
        setRecentlySaved((prev) => {
          const next = { ...prev };
          delete next[s.address_index];
          return next;
        });
        delete savedLabelTimers.current[s.address_index];
      }, SAVED_LABEL_DURATION_MS);
    }
  };

  const queueSaveLabel = (s: SubaddressSummary, value: string) => {
    setLabelEdits((prev) => ({ ...prev, [s.address_index]: value }));
    setLabelErrors((prev) => {
      const next = { ...prev };
      delete next[s.address_index];
      return next;
    });

    if (recentlySaved[s.address_index]) {
      setRecentlySaved((prev) => {
        const next = { ...prev };
        delete next[s.address_index];
        return next;
      });
    }

    if (savedLabelTimers.current[s.address_index]) {
      clearTimeout(savedLabelTimers.current[s.address_index]);
      delete savedLabelTimers.current[s.address_index];
    }

    if (saveTimers.current[s.address_index]) {
      clearTimeout(saveTimers.current[s.address_index]);
    }

    saveTimers.current[s.address_index] = setTimeout(
      () => persistLabel(s, value),
      SAVE_DELAY_MS,
    );
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Monero Subaddresses</DialogTitle>
      <DialogContent dividers>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <TableContainer>
            <Table size="small" stickyHeader>
              <TableHead>
                <TableRow>
                  <TableCell width="10%">Index</TableCell>
                  <TableCell>Address</TableCell>
                  <TableCell width="25%">Label</TableCell>
                  <TableCell align="right" width="15%">
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
                    const isSaving = savingLabels[s.address_index] ?? false;
                    const wasRecentlySaved =
                      recentlySaved[s.address_index] ?? false;
                    const error = labelErrors[s.address_index];
                    const hasPendingChange =
                      labelDraft.trim() !==
                      (
                        subaddressesByIndex[s.address_index]?.label ?? ""
                      ).trim();

                    return (
                      <TableRow key={s.address_index} hover>
                        <TableCell>
                          <Typography variant="body2" color="text.secondary">
                            #{s.address_index}
                          </Typography>
                        </TableCell>
                        <TableCell sx={{ maxWidth: 320 }}>
                          <Box sx={{ maxWidth: 320 }}>
                            <ActionableMonospaceTextBox content={s.address} />
                          </Box>
                        </TableCell>
                        <TableCell>
                          <TextField
                            size="small"
                            fullWidth
                            label="Label"
                            value={labelDraft}
                            error={Boolean(error)}
                            onChange={(e) => queueSaveLabel(s, e.target.value)}
                            onBlur={() => persistLabel(s, labelDraft)}
                            helperText={
                              error
                                ? error
                                : isSaving
                                  ? "Saving…"
                                  : wasRecentlySaved
                                    ? "Saved!"
                                    : hasPendingChange
                                      ? "Auto-save pending…"
                                      : " "
                            }
                          />
                        </TableCell>

                        <TableCell align="right">
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
                        justifyContent: "space-between",
                        flexWrap: "wrap",
                      }}
                    >
                      <Typography variant="body2" color="text.secondary">
                        Labels auto-save after 5 seconds of inactivity or when
                        the field loses focus.
                      </Typography>
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
          </TableContainer>
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}
