import { Box, Chip, Typography } from "@mui/material";
import { CliLog } from "models/cliModel";
import { HashedLog } from "store/features/logsSlice";
import { ReactNode, useMemo, useState } from "react";
import { logsToRawString } from "utils/parseUtils";
import ScrollablePaperTextBox from "./ScrollablePaperTextBox";

function RenderedCliLog({ log }: { log: CliLog }) {
  const { timestamp, level, fields, target } = log;

  const levelColorMap = {
    DEBUG: "#1976d2", // Blue
    INFO: "#388e3c", // Green
    WARN: "#fbc02d", // Yellow
    ERROR: "#d32f2f", // Red
    TRACE: "#8e24aa", // Purple
  };

  return (
    <Box sx={{ display: "flex", flexDirection: "column" }}>
      <Box
        style={{
          display: "flex",
          gap: "0.3rem",
          alignItems: "center",
        }}
      >
        <Chip
          label={level}
          size="small"
          style={{ backgroundColor: levelColorMap[level], color: "white" }}
        />
        {target && (
          <Chip label={target.split("::")[0]} size="small" variant="outlined" />
        )}
        <Chip label={timestamp} size="small" variant="outlined" />
      </Box>
      <Box
        sx={{
          paddingLeft: "1rem",
          paddingTop: "0.2rem",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Typography variant="subtitle2">{fields.message}</Typography>
        {Object.entries(fields).map(([key, value]) => {
          if (key !== "message") {
            return (
              <Typography variant="caption" key={key}>
                {key}: {JSON.stringify(value)}
              </Typography>
            );
          }
          return null;
        })}
      </Box>
    </Box>
  );
}

export default function CliLogsBox({
  label,
  logs,
  topRightButton = null,
  autoScroll = true,
  minHeight,
}: {
  label: string;
  logs: HashedLog[];
  topRightButton?: ReactNode;
  autoScroll?: boolean;
  minHeight?: string;
}) {
  const [searchQuery, setSearchQuery] = useState<string>("");

  const filteredLogs = useMemo(() => {
    if (searchQuery.length === 0) {
      return logs;
    }

    return logs.filter(({ log }) =>
      JSON.stringify(log).toLowerCase().includes(searchQuery.toLowerCase()),
    );
  }, [logs, searchQuery]);

  const rows = useMemo(() => {
    return filteredLogs.map(({ log, hash }) =>
      typeof log === "string" ? (
        <Typography key={hash} component="pre">
          {log}
        </Typography>
      ) : (
        <RenderedCliLog log={log} key={hash} />
      ),
    );
  }, [filteredLogs]);

  const rawStrings = useMemo(
    () => filteredLogs.map(({ log }) => log),
    [filteredLogs],
  );

  return (
    <ScrollablePaperTextBox
      minHeight={minHeight}
      title={label}
      copyValue={logsToRawString(rawStrings)}
      searchQuery={searchQuery}
      setSearchQuery={setSearchQuery}
      topRightButton={topRightButton}
      autoScroll={autoScroll}
      rows={rows}
    />
  );
}
