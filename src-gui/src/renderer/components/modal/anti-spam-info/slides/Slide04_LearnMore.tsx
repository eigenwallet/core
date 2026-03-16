import { Box, Button, Typography } from "@mui/material";
import CheckIcon from "@mui/icons-material/Check";
import { Book, OpenInNew } from "@mui/icons-material";
import SlideTemplate from "../../introduction/slides/SlideTemplate";
import { IntroSlideProps } from "../../introduction/slides/SlideTypes";

function BulletPoint({ children }: { children: React.ReactNode }) {
  return (
    <Box sx={{ display: "flex", alignItems: "flex-start", gap: 1, mb: 1 }}>
      <CheckIcon color="success" fontSize="small" sx={{ mt: 0.3 }} />
      <Typography variant="subtitle1">{children}</Typography>
    </Box>
  );
}

export default function Slide04_LearnMore(props: IntroSlideProps) {
  return (
    <SlideTemplate
      title="What should I know?"
      customContinueButtonText="Got it"
      {...props}
    >
      <Box sx={{ mb: 2 }}>
        <BulletPoint>
          You don't need to deposit extra Bitcoin. Your swap experience doesn't change.
        </BulletPoint>
        <BulletPoint>
          Normal, successful swaps are unaffected.
        </BulletPoint>
        <BulletPoint>
          Makers cannot access the deposit, even if they withhold it.
        </BulletPoint>
        <BulletPoint>
          Makers can still release the deposit after the fact.
        </BulletPoint>
      </Box>

      <p>
        Read more about the background and details of this update in the docs.
        There's an FAQ for quick answers, too.
      </p>


      <Button
        variant="outlined"
        startIcon={<Book />}
        endIcon={<OpenInNew sx={{ fontSize: 14 }} />}
        href="https://docs.eigenwallet.org/advanced/anti_spam_deposit"
        target="_blank"
      >
        Docs + FAQ
      </Button>
    </SlideTemplate>
  );
}
