import { Box } from "@mui/material";
import { Alert, AlertTitle } from "@mui/material";
import { acknowledgeAlert } from "store/features/alertsSlice";
import { useAlerts, useAppDispatch } from "store/hooks";
import { useCallback, useMemo } from "react";

const alertsBoxStyle = {
  display: "flex",
  justifyContent: "center",
  gap: "1rem",
};

export default function ApiAlertsBox() {
  const alerts = useAlerts();
  const dispatch = useAppDispatch();

  const onAcknowledgeAlert = useCallback(
    (id: number) => {
      dispatch(acknowledgeAlert(id));
    },
    [dispatch],
  );

  if (alerts.length === 0) return null;

  return (
    <Box style={alertsBoxStyle}>
      {alerts.map((alert) => (
        <AlertItem
          key={alert.id}
          alert={alert}
          onAcknowledge={onAcknowledgeAlert}
        />
      ))}
    </Box>
  );
}

function AlertItem({
  alert,
  onAcknowledge,
}: {
  alert: { id: number; severity: string; title: string; body: string };
  onAcknowledge: (id: number) => void;
}) {
  const handleClose = useCallback(() => {
    onAcknowledge(alert.id);
  }, [onAcknowledge, alert.id]);

  return (
    <Alert variant="filled" severity={alert.severity} onClose={handleClose}>
      <AlertTitle>{alert.title}</AlertTitle>
      {alert.body}
    </Alert>
  );
}
