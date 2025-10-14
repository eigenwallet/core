import { Box } from "@mui/material";
import { Alert, AlertTitle } from "@mui/material";
import { acknowledgeAlert } from "store/features/alertsSlice";
import { useAlerts, useAppDispatch } from "store/hooks";

export default function ApiAlertsBox() {
  const alerts = useAlerts();
  const dispatch = useAppDispatch();

  function onAcknowledgeAlert(id: number) {
    dispatch(acknowledgeAlert(id));
  }

  if (alerts.length === 0) return null;

  return (
    <Box style={{ display: "flex", justifyContent: "center", gap: "1rem" }}>
      {alerts.map((alert) => (
        <Alert
          variant="filled"
          severity={alert.severity}
          key={alert.id}
          onClose={() => onAcknowledgeAlert(alert.id)}
        >
          <AlertTitle>{alert.title}</AlertTitle>
          {alert.body}
        </Alert>
      ))}
    </Box>
  );
}
