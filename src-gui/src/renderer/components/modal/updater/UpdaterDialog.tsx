import { useEffect, useState } from "react";
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogContentText,
  DialogActions,
  Button,
  LinearProgress,
  Typography,
  LinearProgressProps,
  Box,
  Link,
} from "@mui/material";
import SystemUpdateIcon from "@mui/icons-material/SystemUpdate";
import { check, Update, DownloadEvent } from "@tauri-apps/plugin-updater";
import { useSnackbar } from "notistack";
import { relaunch } from "@tauri-apps/plugin-process";
import { invoke as invokeUnsafe } from "@tauri-apps/api/core";
import { store } from "renderer/store/storeRenderer";
import { NetworkProxyMode } from "store/features/settingsSlice";

const GITHUB_RELEASES_URL = "https://github.com/eigenwallet/core/releases";
const HOMEPAGE_URL = "https://unstoppableswap.net/";

// The updater runs before backend context init, so build the proxy URL
// from persisted settings via Tauri instead of backend state.
async function getSystemTorProxyUrl(): Promise<string | null> {
  const settings = store.getState().settings;
  if (settings.networkProxyMode !== NetworkProxyMode.TorSocks) {
    return null;
  }
  const address = settings.torSocksAddress;
  if (address === null || address === "") {
    throw new Error(
      "Tor Socks proxy is selected but no address is configured. Enter an IPv4 address (e.g. 127.0.0.1:9050) in Settings.",
    );
  }
  return invokeUnsafe<string>("get_updater_proxy_url", { address });
}

interface DownloadProgress {
  contentLength: number | null;
  downloadedBytes: number;
}

function LinearProgressWithLabel(
  props: LinearProgressProps & { label?: string },
) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
      }}
    >
      <Box
        sx={{
          width: "100%",
          mr: 1,
        }}
      >
        <LinearProgress variant="determinate" {...props} />
      </Box>
      <Box
        sx={{
          minWidth: 85,
        }}
      >
        <Typography variant="body2" color="textSecondary">
          {props.label || `${Math.round(props.value ?? 0)}%`}
        </Typography>
      </Box>
    </Box>
  );
}

export default function UpdaterDialog() {
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] =
    useState<DownloadProgress | null>(null);
  const { enqueueSnackbar } = useSnackbar();

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const proxy = await getSystemTorProxyUrl();
        const updateResponse = await check(
          proxy === null ? undefined : { proxy },
        );

        if (cancelled) {
          return;
        }

        console.log("updateResponse", updateResponse);
        setAvailableUpdate(updateResponse);
      } catch (err) {
        if (cancelled) {
          return;
        }

        enqueueSnackbar(`Failed to check for updates: ${err}`, {
          variant: "error",
        });
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [enqueueSnackbar]);

  // If no update is available, don't render the dialog
  if (availableUpdate === null) return null;

  function hideNotification() {
    setAvailableUpdate(null);
  }

  async function handleInstall() {
    if (!availableUpdate) return;

    try {
      await availableUpdate.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          setDownloadProgress({
            contentLength: event.data.contentLength || null,
            downloadedBytes: 0,
          });
        } else if (event.event === "Progress") {
          setDownloadProgress((prev) => {
            if (!prev) return null;
            return {
              contentLength: prev.contentLength,
              downloadedBytes: prev.downloadedBytes + event.data.chunkLength,
            };
          });
        }
      });

      // Once the promise resolves, relaunch the application for the new version to be used
      relaunch();
    } catch (err) {
      enqueueSnackbar(`Failed to install update: ${err}`, {
        variant: "error",
      });
    }
  }

  const isDownloading = downloadProgress !== null;

  const progress =
    isDownloading && downloadProgress.contentLength
      ? Math.round(
          (downloadProgress.downloadedBytes / downloadProgress.contentLength) *
            100,
        )
      : 0;

  return (
    <Dialog
      fullWidth
      maxWidth="sm"
      open={availableUpdate?.available}
      onClose={hideNotification}
    >
      <DialogTitle>Update Available</DialogTitle>
      <DialogContent>
        <DialogContentText>
          A new version (v{availableUpdate.version}) is available. Your current
          version is {availableUpdate.currentVersion}. The update will be
          verified using PGP signature verification to ensure authenticity.
          Alternatively, you can download the update from{" "}
          <Link href={GITHUB_RELEASES_URL} target="_blank">
            GitHub
          </Link>{" "}
          or visit the{" "}
          <Link href={HOMEPAGE_URL} target="_blank">
            download page
          </Link>
          .
          {availableUpdate.body && (
            <>
              <Typography variant="h6" sx={{ mt: 2, mb: 1 }}>
                Release Notes:
              </Typography>
              <Typography
                variant="body2"
                component="div"
                sx={{ whiteSpace: "pre-line" }}
              >
                {availableUpdate.body}
              </Typography>
            </>
          )}
        </DialogContentText>

        {isDownloading && (
          <Box sx={{ mt: 2 }}>
            <LinearProgressWithLabel
              value={progress}
              label={`${(downloadProgress.downloadedBytes / 1024 / 1024).toFixed(1)} MB${
                downloadProgress.contentLength
                  ? ` / ${(downloadProgress.contentLength / 1024 / 1024).toFixed(1)} MB`
                  : ""
              }`}
            />
          </Box>
        )}
      </DialogContent>
      <DialogActions>
        <Button
          variant="text"
          onClick={hideNotification}
          disabled={isDownloading}
        >
          Remind me later
        </Button>
        <Button
          endIcon={<SystemUpdateIcon />}
          variant="contained"
          color="primary"
          onClick={handleInstall}
          disabled={isDownloading}
        >
          {isDownloading ? "DOWNLOADING..." : "INSTALL UPDATE"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
