import { Box, Drawer } from "@mui/material";
import NavigationFooter from "./NavigationFooter";
import NavigationHeader from "./NavigationHeader";

export const drawerWidth = "240px";

const drawerSx = {
  width: drawerWidth,
  flexShrink: 0,
  "& .MuiDrawer-paper": {
    width: drawerWidth,
  },
};

const drawerContentSx = {
  overflow: "auto",
  display: "flex",
  flexDirection: "column",
  justifyContent: "space-between",
  height: "100%",
};

export default function Navigation() {
  return (
    <Drawer variant="permanent" sx={drawerSx}>
      <Box sx={drawerContentSx}>
        <NavigationHeader />
        <NavigationFooter />
      </Box>
    </Drawer>
  );
}
