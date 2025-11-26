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
  TextField,
  Typography,
} from "@mui/material";
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

const SAVED_LABEL_DURATION_MS = 2500;

export default function SubaddressesModal({ open, onClose }: Props) {
  const subaddresses = useMoneroSubaddresses();
  const [newLabel, setNewLabel] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [labelEdits, setLabelEdits] = useState<Record<number, string>>({});
  const [savingLabels, setSavingLabels] = useState<Record<number, boolean>>({});
  const [recentlySaved, setRecentlySaved] = useState<Record<number, boolean>>(
    {},
  );
  const savedLabelTimers = useRef<
    Record<number, ReturnType<typeof setTimeout>>
  >({});

  useEffect(() => {
    return () => {
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
    await createMoneroSubaddress(newLabel.trim());
    return await updateMoneroSubaddresses();
  };

  const handleLabelChange = (s: SubaddressSummary, value: string) => {
    setLabelEdits((prev) => ({ ...prev, [s.address_index]: value }));

    if (recentlySaved[s.address_index]) {
      setRecentlySaved((prev) => {
        const next = { ...prev };
        delete next[s.address_index];
        return next;
      });

      if (savedLabelTimers.current[s.address_index]) {
        clearTimeout(savedLabelTimers.current[s.address_index]);
        delete savedLabelTimers.current[s.address_index];
      }
    }
  };

  const saveLabel = async (s: SubaddressSummary) => {
    const currentValue = (
      subaddressesByIndex[s.address_index]?.label ?? ""
    ).trim();
    const draftValue = (labelEdits[s.address_index] ?? currentValue).trim();

    if (draftValue === currentValue) {
      return;
    }

    setSavingLabels((prev) => ({ ...prev, [s.address_index]: true }));

    await setMoneroSubaddressLabel(
      s.account_index,
      s.address_index,
      draftValue,
    );

    await updateMoneroSubaddresses();

    setLabelEdits((prev) => {
      const next = { ...prev };
      delete next[s.address_index];
      return next;
    });

    setRecentlySaved((prev) => ({ ...prev, [s.address_index]: true }));

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
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Monero Subaddresses</DialogTitle>
      <DialogContent dividers>
        <Box sx={{ display: "flex", gap: 1, alignItems: "center", mb: 2 }}>
          <TextField
            size="small"
            label="New address label (optional)"
            value={newLabel}
            onChange={(e) => setNewLabel(e.target.value)}
            sx={{ flex: 1, maxWidth: 400 }}
          />
          <PromiseInvokeButton
            variant="contained"
            disabled={isCreating}
            onInvoke={createAddress}
            displayErrorSnackbar={true}
            onComplete={() => {
              setNewLabel("");
              setIsCreating(false);
            }}
          >
            Generate Address
          </PromiseInvokeButton>
        </Box>
        {subaddresses.length === 0 ? (
          <Typography color="text.secondary">
            No subaddresses loaded yet.
          </Typography>
        ) : (
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
                  {subaddresses.map((s) => {
                    const labelDraft =
                      labelEdits[s.address_index] ??
                      subaddressesByIndex[s.address_index]?.label ??
                      "";
                    const isSaving = savingLabels[s.address_index] ?? false;
                    const wasRecentlySaved =
                      recentlySaved[s.address_index] ?? false;
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
                          <Box
                            sx={{
                              display: "flex",
                              flexDirection: "column",
                              gap: 0.5,
                            }}
                          >
                            <TextField
                              size="small"
                              fullWidth
                              label="Label"
                              value={labelDraft}
                              onChange={(e) =>
                                handleLabelChange(s, e.target.value)
                              }
                            />
                            <Box
                              sx={{
                                display: "flex",
                                alignItems: "center",
                                gap: 1,
                              }}
                            >
                              <Button
                                size="small"
                                variant="contained"
                                disabled={!hasPendingChange || isSaving}
                                onClick={() => saveLabel(s)}
                              >
                                Save
                              </Button>
                              {isSaving ? (
                                <Typography
                                  variant="caption"
                                  color="text.secondary"
                                >
                                  Saving…
                                </Typography>
                              ) : (
                                wasRecentlySaved && (
                                  <Typography
                                    variant="caption"
                                    sx={{ color: "success.main" }}
                                  >
                                    Saved!
                                  </Typography>
                                )
                              )}
                            </Box>
                          </Box>
                        </TableCell>

                        <TableCell align="right">
                          <Typography variant="body2" color="text.secondary">
                            <PiconeroAmount amount={s.unlocked_balance} />
                          </Typography>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </TableContainer>
          </Box>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}
