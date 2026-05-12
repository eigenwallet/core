import { Typography } from "@mui/material";
import SlideTemplate from "./SlideTemplate";
import imagePath from "assets/walletWithBitcoinAndMonero.png";
import { IntroSlideProps } from "./SlideTypes";

export default function Slide01_GettingStarted(props: IntroSlideProps) {
  return (
    <SlideTemplate title="Getting Started" {...props} imagePath={imagePath}>
      <Typography variant="subtitle1">
        eigenwallet can not only be used to store Bitcoin and Monero funds.
        <br />
        It can also be used to trade your Bitcoin for Monero.
        <br />
        <br />
        All you need is some Bitcoin.
      </Typography>
    </SlideTemplate>
  );
}
