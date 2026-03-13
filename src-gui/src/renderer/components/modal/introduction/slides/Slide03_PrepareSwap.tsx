import { Typography } from "@mui/material";
import SlideTemplate from "./SlideTemplate";
import imagePath from "assets/mockConfigureSwap.svg";
import { IntroSlideProps } from "./SlideTypes";

export default function Slide02_ChooseAMaker(props: IntroSlideProps) {
  return (
    <SlideTemplate
      title="Prepare Swap"
      stepLabel="Step 2"
      {...props}
      imagePath={imagePath}
    >
      <Typography variant="subtitle1">
        To initiate a swap, you need to have some Bitcoin in your wallet.
      </Typography>
    </SlideTemplate>
  );
}
