import { Box, Drawer } from "@mui/material";
import NavigationFooter from "./NavigationFooter";
import NavigationHeader from "./NavigationHeader";
import { drawerWidth } from "./Navigation";

export default function DesktopNavigation() {
  return (
    <Drawer
      variant="permanent"
      sx={{
        width: drawerWidth,
        flexShrink: 0,
        "& .MuiDrawer-paper": {
          width: drawerWidth,
        },
      }}
    >
      <Box
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
  );
}