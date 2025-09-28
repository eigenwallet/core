import { Box } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { buyXmr } from "renderer/rpc";

export default function InitPage() {
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
