import { useState, useEffect } from "react";
import { store } from "renderer/store/storeRenderer";
import { useActiveSwapInfo } from "store/hooks";
import { logsToRawString } from "utils/parseUtils";
import { getLogsOfSwap, redactLogs } from "renderer/rpc";
import { parseCliLogString } from "models/cliModel";
import logger from "utils/logger";
import { submitFeedbackViaHttp } from "renderer/api";
import { addFeedbackId } from "store/features/conversationsSlice";
import { AttachmentInput } from "models/apiModel";
import { useSnackbar } from "notistack";
import { HashedLog, hashLogs } from "store/features/logsSlice";

export const MAX_FEEDBACK_LENGTH = 4000;

interface FeedbackInputState {
  bodyText: string;
  selectedSwap: string | null;
  attachDaemonLogs: boolean;
  isSwapLogsRedacted: boolean;
  isDaemonLogsRedacted: boolean;
}

interface FeedbackLogsState {
  swapLogs: HashedLog[];
  daemonLogs: HashedLog[];
}

const initialInputState: FeedbackInputState = {
  bodyText: "",
  selectedSwap: null,
  attachDaemonLogs: true,
  isSwapLogsRedacted: false,
  isDaemonLogsRedacted: false,
};

const initialLogsState: FeedbackLogsState = {
  swapLogs: [],
  daemonLogs: [],
};

export function useFeedback() {
  const currentSwapId = useActiveSwapInfo();
  const { enqueueSnackbar } = useSnackbar();

  const [inputState, setInputState] = useState<FeedbackInputState>({
    ...initialInputState,
    selectedSwap: currentSwapId?.swap_id || null,
  });
  const [logsState, setLogsState] =
    useState<FeedbackLogsState>(initialLogsState);
  const [error, setError] = useState<string | null>(null);

  // Fetch swap logs when selection changes
  useEffect(() => {
    if (inputState.selectedSwap === null) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- clear when deselected
      setLogsState((prev) => ({ ...prev, swapLogs: [] }));
      return;
    }

    getLogsOfSwap(inputState.selectedSwap, inputState.isSwapLogsRedacted)
      .then((response) => {
        const parsedLogs = response.logs.map(parseCliLogString);
        setLogsState((prev) => ({
          ...prev,
          swapLogs: hashLogs(parsedLogs),
        }));
        setError(null);
      })
      .catch((e) => {
        logger.error(`Failed to fetch swap logs: ${e}`);
        setLogsState((prev) => ({ ...prev, swapLogs: [] }));
        setError(`Failed to fetch swap logs: ${e}`);
      });
  }, [inputState.selectedSwap, inputState.isSwapLogsRedacted]);

  // Fetch/process daemon logs when settings change
  useEffect(() => {
    if (!inputState.attachDaemonLogs) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- clear when detached
      setLogsState((prev) => ({ ...prev, daemonLogs: [] }));
      return;
    }

    const hashedLogs = store.getState().logs?.state.logs ?? [];

    if (inputState.isDaemonLogsRedacted) {
      const logs = hashedLogs.map((h) => h.log);
      redactLogs(logs)
        .then((redactedLogs) => {
          setLogsState((prev) => ({
            ...prev,
            daemonLogs: hashLogs(redactedLogs),
          }));
          setError(null);
        })
        .catch((e) => {
          logger.error(`Failed to redact daemon logs: ${e}`);
          setLogsState((prev) => ({ ...prev, daemonLogs: [] }));
          setError(`Failed to redact daemon logs: ${e}`);
        });
    } else {
      setLogsState((prev) => ({ ...prev, daemonLogs: hashedLogs }));
    }
  }, [inputState.attachDaemonLogs, inputState.isDaemonLogsRedacted]);

  const clearState = () => {
    setInputState(initialInputState);
    setLogsState(initialLogsState);
    setError(null);
  };

  const submitFeedback = async () => {
    if (inputState.bodyText.length === 0) {
      setError("Please enter a message");
      throw new Error("User did not enter a message");
    }

    const attachments: AttachmentInput[] = [];
    // Add swap logs as an attachment
    if (logsState.swapLogs.length > 0) {
      attachments.push({
        key: `swap_logs_${inputState.selectedSwap}.txt`,
        content: logsToRawString(logsState.swapLogs.map((h) => h.log)),
      });
    }

    // Handle daemon logs
    if (logsState.daemonLogs.length > 0) {
      attachments.push({
        key: "daemon_logs.txt",
        content: logsToRawString(logsState.daemonLogs.map((h) => h.log)),
      });
    }

    // Call the updated API function
    const feedbackId = await submitFeedbackViaHttp(
      inputState.bodyText,
      attachments,
    );

    enqueueSnackbar("Feedback submitted successfully", {
      variant: "success",
    });

    // Dispatch only the ID
    store.dispatch(addFeedbackId(feedbackId));
  };

  return {
    input: inputState,
    setInputState,
    logs: logsState,
    error,
    clearState,
    submitFeedback,
  };
}
