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
import type { SubaddressSummary } from "models/tauriModel";
import { useState } from "react";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { PiconeroAmount } from "renderer/components/other/Units";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import {
  createMoneroSubaddress,
  setMoneroSubaddressLabel,
  updateMoneroSubaddresses,
} from "renderer/rpc";
import { useAppSelector } from "store/hooks";

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function SubaddressesModal({ open, onClose }: Props) {
  const subaddresses = useAppSelector((s) => s.wallet.state.subaddresses);
  const [newLabel, setNewLabel] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [editingKey, setEditingKey] = useState<number | undefined>();
  const [editLabel, setEditLabel] = useState("");

  const createAddress = async () => {
    setIsCreating(true);
    await createMoneroSubaddress(newLabel.trim());
    return await updateMoneroSubaddresses();
  };

  const editAddressLabel = async (s: SubaddressSummary) => {
    await setMoneroSubaddressLabel(
      s.account_index,
      s.address_index,
      editLabel.trim(),
    );
    return await updateMoneroSubaddresses();
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
            onEnded={() => {
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
          <List dense>
            {subaddresses.map((s) => (
              <ListItem key={s.address_index} divider sx={{ display: "block" }}>
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
                      {editingKey === s.address_index ? (
                        <>
                          <TextField
                            size="small"
                            label="Label"
                            value={editLabel}
                            onChange={(e) => setEditLabel(e.target.value)}
                          />
                          <PromiseInvokeButton
                            size="small"
                            variant="contained"
                            onInvoke={() => editAddressLabel(s)}
                            onEnded={() => {
                              setEditingKey(undefined);
                              setEditLabel("");
                            }}
                          >
                            Save
                          </PromiseInvokeButton>
                          <Button
                            size="small"
                            onClick={() => {
                              setEditingKey(undefined);
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
                              setEditingKey(s.address_index);
                              setEditLabel(s.label);
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
