import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { Alert } from "models/apiModel";
import { fnv1a } from "utils/hash";

export interface AlertsSlice {
  alerts: Alert[];
  /// The ids of the alerts that have been acknowledged
  /// by the user and should not be shown again
  acknowledgedAlerts: AcknowledgementKey[];
}

const initialState: AlertsSlice = {
  alerts: [],
  acknowledgedAlerts: [],
};

/// We use the key in combination with the fnv1a hash of the title
/// to uniquely identify an alert
///
/// If the title changes, the hash will change and the alert will be shown again
interface AcknowledgementKey {
  id: number;
  titleHash: string;
}

const alertsSlice = createSlice({
  name: "alerts",
  initialState,
  reducers: {
    setAlerts(slice, action: PayloadAction<Alert[]>) {
      slice.alerts = action.payload;
    },
    acknowledgeAlert(slice, action: PayloadAction<number>) {
      const alertTitle = slice.alerts.find(
        (alert) => alert.id === action.payload,
      )?.title;

      // If we cannot find the alert, we cannot acknowledge it
      if (alertTitle != null) {
        slice.acknowledgedAlerts.push({
          id: action.payload,
          titleHash: fnv1a(alertTitle),
        });
      }
    },
  },
});

export const { setAlerts, acknowledgeAlert } = alertsSlice.actions;
export default alertsSlice.reducer;
