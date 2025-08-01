import { useTheme, useMediaQuery } from "@mui/material";

export const useIsMobile = () => {
  return true;
  const theme = useTheme();
  return useMediaQuery(theme.breakpoints.down("md"));
};
