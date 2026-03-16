import { Typography } from "@mui/material";
import SlideTemplate from "../../introduction/slides/SlideTemplate";
import { IntroSlideProps } from "../../introduction/slides/SlideTypes";

export default function Slide01_WhatsNew(props: IntroSlideProps) {
  return (
    <SlideTemplate title="Protocol Update v4.0.0" {...props}>
      <Typography variant="subtitle1">
        There has been an update to the eigenwallet protocol.
        <br />
        <br />
        Makers can now require a small anti-spam deposit to protect themselves
        against spammers.
        <br />
        <br />
        You'll see the deposit amount when choosing an offer and again when confirming.
      </Typography>
    </SlideTemplate>
  );
}
