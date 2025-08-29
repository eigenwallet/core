import {
  Dialog,
  DialogProps,
  Slide,
  useMediaQuery,
  useTheme,
} from "@mui/material";
import { TransitionProps } from "@mui/material/transitions";
import { forwardRef, ReactElement, Ref } from "react";

const Transition = forwardRef(function Transition(
  props: TransitionProps & {
    children: ReactElement;
  },
  ref: Ref<unknown>,
) {
  return <Slide direction="up" ref={ref} {...props} />;
});

interface MobileDialogProps extends DialogProps {
  children: React.ReactNode;
}

export default function MobileDialog({
  children,
  ...props
}: MobileDialogProps) {
  const theme = useTheme();
  const isMobile = useMediaQuery(theme.breakpoints.down("md"));

  return (
    <Dialog
      {...props}
      fullScreen={isMobile}
      slots={{
        transition: isMobile ? Transition : undefined,
      }}
      sx={{
        ...(isMobile && {
          "& .MuiDialog-paper": {
            margin: 0,
            width: "100%",
            height: "100%",
            maxHeight: "100%",
            maxWidth: "100%",
            borderRadius: 0,
          },
        }),
        ...props.sx,
      }}
    >
      {children}
    </Dialog>
  );
}
