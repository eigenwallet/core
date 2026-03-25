import { Link, SlideProps, Typography } from "@mui/material";
import SlideTemplate from "./SlideTemplate";
import imagePath from "assets/mockHistoryPage.svg";
import ExternalLink from "renderer/components/other/ExternalLink";
import { IntroSlideProps } from "./SlideTypes";

export default function Slide05_KeepAnEyeOnYourSwaps(props: IntroSlideProps) {
  return (
    <SlideTemplate
      title="Monitor Your Swaps"
      stepLabel="Step 3"
      {...props}
      imagePath={imagePath}
    >
      <Typography>
        Sometimes a swap needs to be refunded. Just have the app open at any
        point during the refund period.
      </Typography>
      <Typography>
        <ExternalLink href="https://docs.unstoppableswap.net/usage/first_swap">
          Learn more about atomic swaps
        </ExternalLink>
      </Typography>
    </SlideTemplate>
  );
}
