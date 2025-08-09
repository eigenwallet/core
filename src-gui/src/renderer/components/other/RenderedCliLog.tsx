import { Box, Chip, Typography } from "@mui/material";
import { CliLog } from "models/cliModel";
import { ReactNode, useMemo, useState } from "react";
import { logsToRawString } from "utils/parseUtils";
import ScrollablePaperTextBox from "./ScrollablePaperTextBox";

function RenderedCliLog({ log }: { log: CliLog }) {
  const {
    timestamp,
    level,
    fields = {},
    target,
  } = log as CliLog & {
    fields?: Record<string, unknown>;
  };

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
        <Typography variant="subtitle2">
          {(fields as any)?.message ?? ""}
        </Typography>
        {Object.entries(fields ?? {}).map(([key, value]) => {
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
  logPairs,
  topRightButton = null,
  autoScroll = false,
  minHeight,
  topAdornment,
  onReachTop,
}: {
  label: string;
  logs: (CliLog | string)[];
  logPairs?: [number, CliLog | string][];
  topRightButton?: ReactNode;
  autoScroll?: boolean;
  minHeight?: string;
  topAdornment?: ReactNode;
  onReachTop?: () => void;
}) {
  const [searchQuery, setSearchQuery] = useState<string>("");

  // Build view dataset from pairs if provided; fallback to logs
  const memoizedPairs = useMemo(() => {
    const pairs: [number, CliLog | string][] = logPairs
      ? logPairs
      : logs.map((log, idx) => [idx, log]);
    if (searchQuery.length === 0) return pairs;
    return pairs.filter(([, log]) =>
      JSON.stringify(log).toLowerCase().includes(searchQuery.toLowerCase()),
    );
  }, [logs, logPairs, searchQuery]);

  return (
    <ScrollablePaperTextBox
      minHeight={minHeight}
      title={label}
      copyValue={logsToRawString(memoizedPairs.map(([, log]) => log))}
      searchQuery={searchQuery}
      setSearchQuery={setSearchQuery}
      topRightButton={topRightButton}
      autoScroll={autoScroll}
      topAdornment={topAdornment}
      onReachTop={onReachTop}
      rows={memoizedPairs.map(([displayIdx, log]) =>
        typeof log === "string" ? (
          <Typography key={`${displayIdx}-${log}`} component="pre">
            [{displayIdx}] {log}
          </Typography>
        ) : (
          <Box
            key={`${displayIdx}-${JSON.stringify(log)}`}
            sx={{ display: "flex", gap: 1 }}
          >
            <Typography variant="caption" sx={{ width: 80 }}>
              [{displayIdx}]
            </Typography>
            <RenderedCliLog log={log} />
          </Box>
        ),
      )}
    />
  );
}
