import { Box, Typography } from "@mui/material";
import SlideTemplate from "../../introduction/slides/SlideTemplate";
import { IntroSlideProps } from "../../introduction/slides/SlideTypes";

export default function Slide03_WhatIfICancel(props: IntroSlideProps) {
  return (
    <SlideTemplate title="What if a swap is refunded?" {...props}>
      <Box
        sx={{
          my: 2,
          borderRadius: 1,
          overflow: "hidden",
          display: "flex",
          height: 40,
        }}
      >
        <Box
          sx={{
            bgcolor: "success.main",
            flex: 95,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Typography variant="body2" fontWeight="bold">
            95% immediate refund
          </Typography>
        </Box>
        <Box
          sx={{
            bgcolor: "warning.main",
            flex: 5,
            minWidth: 90,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Typography variant="body2" fontWeight="bold">
            5% deposit
          </Typography>
        </Box>
      </Box>
      <Typography variant="subtitle1">
        If an anti-spam deposit was agreed, you'll get your Bitcoin back in two
        steps:
      </Typography>
      <Typography component="ol" variant="subtitle1" sx={{ px: 4, mt: 1 }}>
        <li>
          Everything but the deposit is refunded instantly, just like before.{" "}
          <i>(95% in this example)</i>
        </li>
        <li>
          We have to wait for ~30min to reclaim the anti-spam deposit.{" "}
          <i>(5% in this example)</i>
        </li>
      </Typography>
      <Typography variant="subtitle1" sx={{ mt: 1 }}>
        During these 30 minutes the maker may withhold the deposit. They do this
        if they think you are spamming them. But this only happens in rare
        circumstances.
      </Typography>
      <br />
      <Typography variant="caption" color="text.secondary" sx={{ mt: 1 }}>
        The percentage differs between offers. This is only an example.
      </Typography>
    </SlideTemplate>
  );
}
