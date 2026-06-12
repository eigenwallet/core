import {
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  FormControlLabel,
  List,
  ListItem,
  ListItemIcon,
  ListItemText,
  Typography,
} from "@mui/material";
import CircleIcon from "@mui/icons-material/Circle";
import { useState } from "react";
import PromiseInvokeButton from "../PromiseInvokeButton";

type SwapCancelAlertProps = {
  open: boolean;
  onClose: () => void;
  onSuspend: (disableAutoResume: boolean) => Promise<void>;
};

export default function SwapSuspendAlert({
  open,
  onClose,
  onSuspend,
}: SwapCancelAlertProps) {
  const [disableAutoResume, setDisableAutoResume] = useState(false);

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Suspend running swap?</DialogTitle>
      <DialogContent>
        <DialogContentText component="div">
          <List dense>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText primary="The swap and any network requests between you and the maker will be paused until you resume" />
            </ListItem>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText
                primary={
                  <>
                    Refund timelocks will <strong>not</strong> be paused. They
                    will continue to count down until they expire
                  </>
                }
              />
            </ListItem>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText primary="You can monitor the timelock on the history page" />
            </ListItem>
            <ListItem sx={{ pl: 0 }}>
              <ListItemIcon sx={{ minWidth: "30px" }}>
                <CircleIcon sx={{ fontSize: "8px" }} />
              </ListItemIcon>
              <ListItemText primary="If the refund timelock expires, a refund will be initiated in the background. This still requires the app to be running." />
            </ListItem>
          </List>
        </DialogContentText>
        <FormControlLabel
          control={
            <Checkbox
              checked={disableAutoResume}
              onChange={(event) => setDisableAutoResume(event.target.checked)}
            />
          }
          label="Don't auto resume on startup"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="primary">
          No
        </Button>
        <PromiseInvokeButton
          color="primary"
          onSuccess={onClose}
          onInvoke={() => onSuspend(disableAutoResume)}
          contextRequirement={false}
        >
          Suspend
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
