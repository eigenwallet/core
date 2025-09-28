import { Box, Skeleton } from "@mui/material";
import { buyXmr, isThereASwapRunning } from "renderer/rpc";
import { useEffect } from "react";
import { useSnackbar } from "notistack";

export default function InitPage() {
  const { enqueueSnackbar } = useSnackbar();

  async function init() {
    try {
      // We only call buyXmr if there is no swap running
      // otherwise we will get an error but keep retrying
      if (!(await isThereASwapRunning())) {
        await buyXmr();
      }
    } catch (error) {
      enqueueSnackbar(error as string, {
        variant: "error",
      });

      setTimeout(() => {
        init();
      }, 5000);
    }
  }

  useEffect(() => {
    init();
  }, []);

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <Skeleton variant="rectangular" height={"15vh"} />
      <Box sx={{ height: "25vh" }}></Box>
    </Box>
  );
}
