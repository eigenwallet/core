import { useState } from "react";
import {
  Box,
  Button,
  IconButton,
  MenuItem,
  Paper,
  Select,
  Switch,
  Typography,
} from "@mui/material";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import { scenarios, MockScenario, MOCK_SWAP_ID, getMockLockBitcoinApproval } from "dev/mockSwapEvents";
import { useAppDispatch } from "store/hooks";
import { approvalEventReceived } from "store/features/rpcSlice";
import {
  swapProgressEventReceived,
  swapReset,
  setMockOnlyDisableTauriCallsOnSwapProgress,
} from "store/features/swapSlice";

export default function MockSwapControls() {
  const dispatch = useAppDispatch();
  const [scenario, setScenario] = useState<MockScenario | null>(null);
  const [index, setIndex] = useState(0);

  const enabled = scenario !== null;
  const total = scenario ? scenarios[scenario].length : 0;

  const dispatchMockState = (mockScenario: MockScenario, eventIndex: number) => {
    const event = scenarios[mockScenario][eventIndex];
    dispatch(
      swapProgressEventReceived({
        swap_id: MOCK_SWAP_ID,
        event,
      }),
    );
  };

  const handleMockConfirmation = () => {
    dispatch(approvalEventReceived(getMockLockBitcoinApproval(scenario)));
  };

  const handleToggle = (checked: boolean) => {
    if (checked) {
      const firstScenario = Object.keys(scenarios)[0] as MockScenario;
      setScenario(firstScenario);
      setIndex(0);
      dispatch(setMockOnlyDisableTauriCallsOnSwapProgress(true));
      dispatchMockState(firstScenario, 0);
    } else {
      setScenario(null);
      setIndex(0);
      dispatch(setMockOnlyDisableTauriCallsOnSwapProgress(false));
      dispatch(swapReset());
    }
  };

  const handleScenarioChange = (newScenario: MockScenario) => {
    setScenario(newScenario);
    setIndex(0);
    dispatchMockState(newScenario, 0);
  };

  const prev = () => {
    if (!scenario || index === 0) return;
    const newIndex = index - 1;
    setIndex(newIndex);
    dispatchMockState(scenario, newIndex);
  };

  const next = () => {
    if (!scenario || index >= total - 1) return;
    const newIndex = index + 1;
    setIndex(newIndex);
    dispatchMockState(scenario, newIndex);
  };

  const currentStateName = scenario ? scenarios[scenario][index].type : null;

  return (
    <Paper variant="outlined" sx={{ p: 1.5, bgcolor: "action.hover" }}>
      <Box sx={{ display: "flex", alignItems: "center", gap: 1, flexWrap: "wrap" }}>
        <Switch
          size="small"
          checked={enabled}
          onChange={(e) => handleToggle(e.target.checked)}
        />
        <Typography variant="caption" color="text.secondary">
          Mock
        </Typography>
        <Select
          size="small"
          value={scenario ?? ""}
          onChange={(e) => handleScenarioChange(e.target.value as MockScenario)}
          disabled={!enabled}
          sx={{ minWidth: 180 }}
        >
          {Object.keys(scenarios).map((key) => (
            <MenuItem key={key} value={key}>
              {key}
            </MenuItem>
          ))}
        </Select>
        {enabled && (
          <>
            <IconButton size="small" onClick={prev} disabled={index === 0}>
              <ChevronLeftIcon fontSize="small" />
            </IconButton>
            <Typography variant="body2" sx={{ minWidth: 40, textAlign: "center" }}>
              {index + 1}/{total}
            </Typography>
            <IconButton size="small" onClick={next} disabled={index === total - 1}>
              <ChevronRightIcon fontSize="small" />
            </IconButton>
            <Typography
              variant="body2"
              sx={{
                fontFamily: "monospace",
                bgcolor: "background.paper",
                px: 1,
                py: 0.5,
                borderRadius: 1,
                border: 1,
                borderColor: "divider",
              }}
            >
              {currentStateName}
            </Typography>
          </>
        )}
        <Button size="small" variant="outlined" onClick={handleMockConfirmation}>
          Mock Confirmation
        </Button>
      </Box>
    </Paper>
  );
}
