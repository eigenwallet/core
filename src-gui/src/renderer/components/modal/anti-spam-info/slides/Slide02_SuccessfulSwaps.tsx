import { Typography } from "@mui/material";
import SlideTemplate from "../../introduction/slides/SlideTemplate";
import { IntroSlideProps } from "../../introduction/slides/SlideTypes";

export default function Slide02_SuccessfulSwaps(props: IntroSlideProps) {
  return (
    <SlideTemplate title="Successful Swaps Are Unaffected" {...props}>
      <Typography variant="subtitle1">
        If your swap completes successfully, you receive your full Monero
        amount. Nothing changes.
        <br />
        <br />
        The deposit only matters if a swap is refunded.
      </Typography>
    </SlideTemplate>
  );
}
