import { Typography } from "@mui/material";
import SlideTemplate from "./SlideTemplate";
import imagePath from "assets/mockMakerSelection.svg";
import { IntroSlideProps } from "./SlideTypes";

export default function Slide02_ChooseAMaker(props: IntroSlideProps) {
  return (
    <SlideTemplate
      title="Choose a Maker"
      stepLabel="Step 1"
      {...props}
      imagePath={imagePath}
    >
      <Typography variant="subtitle1">
        To start a swap, choose a maker. Each maker offers different exchange
        rates and limits.
      </Typography>
    </SlideTemplate>
  );
}
