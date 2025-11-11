import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  List,
  ListItem,
  ListItemText,
  TextField,
  Typography,
} from "@mui/material";
import { useState } from "react";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { PiconeroAmount } from "renderer/components/other/Units";
import {
  createMoneroSubaddress,
  getMoneroSubAddresses,
  setMoneroSubaddressLabel,
} from "renderer/rpc";
import { setSubaddresses } from "store/features/walletSlice";
import { useAppDispatch, useAppSelector } from "store/hooks";

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function SubaddressesModal({ open, onClose }: Props) {
  const dispatch = useAppDispatch();
  const subaddresses = useAppSelector((s) => s.wallet.state.subaddresses);
  const [newLabel, setNewLabel] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [editLabel, setEditLabel] = useState("");

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
          <Button
            variant="contained"
            disabled={isCreating}
            onClick={async () => {
              try {
                setIsCreating(true);
                await createMoneroSubaddress(newLabel.trim());
                const subs = await getMoneroSubAddresses();
                dispatch(setSubaddresses(subs));
                setNewLabel("");
              } finally {
                setIsCreating(false);
              }
            }}
          >
            Generate Address
          </Button>
        </Box>
        {subaddresses.length === 0 ? (
          <Typography color="text.secondary">
            No subaddresses loaded yet.
          </Typography>
        ) : (
          <List dense>
            {subaddresses.map((s) => (
              <ListItem
                key={`${s.account_index}-${s.address_index}`}
                divider
                sx={{ display: "block" }}
              >
                <ListItemText
                  primary={<ActionableMonospaceTextBox content={s.address} />}
                  secondary={
                    <Box
                      sx={{
                        display: "flex",
                        gap: 2,
                        flexWrap: "wrap",
                        mt: 0.5,
                      }}
                    >
                      <Typography variant="body2" color="text.secondary">
                        Address #{s.address_index}
                      </Typography>
                      {editingKey ===
                        `${s.account_index}-${s.address_index}` ? (
                        <>
                          <TextField
                            size="small"
                            label="Label"
                            value={editLabel}
                            onChange={(e) => setEditLabel(e.target.value)}
                          />
                          <Button
                            size="small"
                            variant="contained"
                            onClick={async () => {
                              await setMoneroSubaddressLabel(
                                s.account_index,
                                s.address_index,
                                editLabel.trim(),
                              );
                              const subs = await getMoneroSubAddresses();
                              dispatch(setSubaddresses(subs));
                              setEditingKey(null);
                              setEditLabel("");
                            }}
                          >
                            Save
                          </Button>
                          <Button
                            size="small"
                            onClick={() => {
                              setEditingKey(null);
                              setEditLabel("");
                            }}
                          >
                            Cancel
                          </Button>
                        </>
                      ) : (
                        <>
                          {s.label && (
                            <Typography variant="body2" color="text.secondary">
                              Label: {s.label}
                            </Typography>
                          )}
                          <Button
                            size="small"
                            onClick={() => {
                              // Drop the "#<index>" prefix if present
                              const cleaned = s.label.replace(
                                new RegExp(`^#${s.address_index}\\s?`),
                                "",
                              );
                              setEditingKey(
                                `${s.account_index}-${s.address_index}`,
                              );
                              setEditLabel(cleaned);
                            }}
                          >
                            Edit label
                          </Button>
                        </>
                      )}
                      <Typography variant="body2" color="text.secondary">
                        Unlocked Balance:{" "}
                        <PiconeroAmount amount={s.unlocked_balance} />
                      </Typography>

                      <Typography variant="body2" color="text.secondary">
                        Tx count: {s.tx_count}
                      </Typography>
                    </Box>
                  }
                />
              </ListItem>
            ))}
          </List>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}
