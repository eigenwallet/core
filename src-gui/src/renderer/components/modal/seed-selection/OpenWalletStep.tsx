import {
  Box,
  Divider,
  List,
  ListItem,
  ListItemButton,
  ListItemText,
  Typography,
} from "@mui/material";
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";

export default function OpenWalletStep({
  walletPath,
  setWalletPath,
  recentWallets,
}: {
  walletPath: string;
  setWalletPath: (path: string) => void;
  recentWallets: string[];
}) {
  const [isDragging, setIsDragging] = useState(false);

  // Tauri delivers dropped file paths through the webview drag-drop event
  // rather than the DOM, so we subscribe to it while this step is mounted.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop") {
          setIsDragging(false);
          const path = event.payload.paths[0];
          if (path) setWalletPath(path);
        } else if (event.payload.type === "leave") {
          setIsDragging(false);
        } else {
          setIsDragging(true);
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [setWalletPath]);

  const selectWalletFile = async () => {
    const selected = await open({ multiple: false, directory: false });
    if (selected) setWalletPath(selected);
  };

  return (
    <Box sx={{ gap: 2, display: "flex", flexDirection: "column" }}>
      <Box
        onClick={selectWalletFile}
        sx={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 1,
          py: 4,
          px: 2,
          cursor: "pointer",
          borderRadius: 2,
          border: "2px dashed",
          borderColor: isDragging ? "primary.main" : "divider",
          backgroundColor: isDragging ? "action.hover" : "transparent",
          transition: "border-color 0.15s, background-color 0.15s",
          "&:hover": { borderColor: "primary.main" },
        }}
      >
        <FolderOpenIcon sx={{ fontSize: 40, color: "text.secondary" }} />
        <Typography variant="body2" color="text.secondary">
          Drag a wallet file here, or click to browse
        </Typography>
      </Box>

      {recentWallets.length > 0 && (
        <Box
          sx={{
            border: 1,
            borderColor: "divider",
            borderRadius: 1,
            maxHeight: 200,
            overflowY: "scroll",
            "&::-webkit-scrollbar": {
              display: "block !important",
              width: "8px !important",
            },
            "&::-webkit-scrollbar-track": {
              display: "block !important",
              background: "rgba(255,255,255,.1) !important",
              borderRadius: "4px",
            },
            "&::-webkit-scrollbar-thumb": {
              display: "block !important",
              background: "rgba(255,255,255,.6) !important",
              borderRadius: "4px",
              minHeight: "20px !important",
            },
            "&::-webkit-scrollbar-thumb:hover": {
              background: "rgba(255,255,255,.8) !important",
            },
            "&::-webkit-scrollbar-corner": {
              background: "transparent !important",
            },
            scrollbarWidth: "thin",
            scrollbarColor: "rgba(255,255,255,.6) rgba(255,255,255,.1)",
          }}
        >
          <List disablePadding>
            {recentWallets.map((path, index) => (
              <Box key={path}>
                <ListItem disablePadding>
                  <ListItemButton
                    selected={walletPath === path}
                    onClick={() => setWalletPath(path)}
                  >
                    <ListItemText
                      primary={path.split(/[/\\]/).pop() || path}
                      secondary={path}
                      primaryTypographyProps={{
                        fontWeight: walletPath === path ? 600 : 400,
                        fontSize: "0.9rem",
                      }}
                      secondaryTypographyProps={{
                        fontSize: "0.75rem",
                        sx: {
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        },
                      }}
                    />
                  </ListItemButton>
                </ListItem>
                {index < recentWallets.length - 1 && <Divider />}
              </Box>
            ))}
          </List>
        </Box>
      )}
    </Box>
  );
}
