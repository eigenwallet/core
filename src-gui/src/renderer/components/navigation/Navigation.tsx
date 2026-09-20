import { Box, Drawer, Fab, Theme, useMediaQuery } from "@mui/material";
import MenuIcon from "@mui/icons-material/Menu";
import { useState } from "react";
import NavigationFooter from "./NavigationFooter";
import NavigationHeader from "./NavigationHeader";

export const drawerWidth = "240px";

export default function Navigation() {
  const isMobile = useMediaQuery((theme: Theme) =>
    theme.breakpoints.down("sm"),
  );
  const [open, setOpen] = useState(false);

  return (
    <>
      {isMobile && (
        <Fab
          size="small"
          onClick={() => setOpen(true)}
          sx={{
            position: "fixed",
            top: 8,
            left: 8,
            zIndex: (theme) => theme.zIndex.drawer - 1,
            bgcolor: "background.paper",
            color: "text.primary",
            "&:hover": {
              bgcolor: "background.paper",
            },
          }}
        >
          <MenuIcon />
        </Fab>
      )}
      <Drawer
        variant={isMobile ? "temporary" : "permanent"}
        open={!isMobile || open}
        onClose={() => setOpen(false)}
        sx={{
          width: drawerWidth,
          flexShrink: 0,
          "& .MuiDrawer-paper": {
            width: drawerWidth,
          },
        }}
      >
        <Box
          onClick={() => isMobile && setOpen(false)}
          sx={{
            overflow: "auto",
            display: "flex",
            flexDirection: "column",
            justifyContent: "space-between",
            height: "100%",
          }}
        >
          <NavigationHeader />
          <NavigationFooter />
        </Box>
      </Drawer>
    </>
  );
}
