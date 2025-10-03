import { Box, Button, Skeleton } from "@mui/material";
import { buyXmr } from "renderer/rpc";
import { useSnackbar } from "notistack";

export default function InitPage() {
  const { enqueueSnackbar } = useSnackbar();

  async function init() {
    try {
      await buyXmr();
    } catch (error) {
      enqueueSnackbar(error as string, {
        variant: "error",
      });
    }
  }

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 2,
        flex: 1,
        position: "relative",
        cursor: "pointer",
      }}
      onClick={init}
    >
      <Skeleton variant="rounded" sx={{ flex: 1, minHeight: "20vh" }} />
      <Box
        sx={{
          flex: 1,
          display: "flex",
          flexDirection: "column",
          gap: 2,
          borderRadius: 1,
        }}
      >
        {[...Array(4)].map((_, index) => (
          <Box
            sx={{ display: "flex", alignItems: "center", gap: 2 }}
            key={index}
          >
            <Skeleton variant="circular" width={40} height={40} />
            <Skeleton
              variant="rounded"
              animation="wave"
              height={"7vh"}
              sx={{ flex: 1 }}
            />
          </Box>
        ))}
      </Box>

      <Box
        sx={{
          position: "absolute",
          top: 0,
          left: 0,
          right: 0,
          bottom: 0,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: "15%",
          margin: "-1rem",
          borderRadius: "1rem",
          pointerEvents: "none",
          backdropFilter: "blur(1px)",
        }}
      >
        <Button
          variant="contained"
          onClick={init}
          size="large"
          sx={{ pointerEvents: "auto" }}
        >
          Click to view offers
        </Button>
      </Box>
    </Box>
  );
}
