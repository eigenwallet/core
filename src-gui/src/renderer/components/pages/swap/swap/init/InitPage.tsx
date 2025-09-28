import { Box } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { buyXmr, getCurrentSwapId, isThereASwapRunning } from "renderer/rpc";
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
    <Box style={{ display: "flex", justifyContent: "center" }}>
      <PromiseInvokeButton
        variant="contained"
        color="primary"
        size="large"
        sx={{ marginTop: 1 }}
        endIcon={<PlayArrowIcon />}
        onInvoke={buyXmr}
        displayErrorSnackbar
      >
        Continue
      </PromiseInvokeButton>
    </Box>
  );
}
