import { Modal } from "@mui/material";
import { useState } from "react";
import Slide01_WhatsNew from "./slides/Slide01_WhatsNew";
import Slide02_SuccessfulSwaps from "./slides/Slide02_SuccessfulSwaps";
import Slide03_WhatIfICancel from "./slides/Slide03_WhatIfICancel";
import Slide04_LearnMore from "./slides/Slide04_LearnMore";
import { setUserHasSeenAntiSpamInfo } from "store/features/settingsSlice";
import { useAppDispatch, useSettings } from "store/hooks";

export default function AntiSpamInfoModal() {
  const userHasSeenAntiSpamInfo = useSettings((s) => s.userHasSeenAntiSpamInfo);

  const dispatch = useAppDispatch();

  const [open, setOpen] = useState<boolean>(!userHasSeenAntiSpamInfo);

  const handleClose = () => {
    setOpen(false);
    dispatch(setUserHasSeenAntiSpamInfo(true));
  };

  const [currentSlideIndex, setCurrentSlideIndex] = useState(0);

  const handleContinue = () => {
    if (currentSlideIndex === slideComponents.length - 1) {
      handleClose();
      return;
    }
    setCurrentSlideIndex((i) => i + 1);
  };

  const handlePrevious = () => {
    if (currentSlideIndex === 0) {
      return;
    }
    setCurrentSlideIndex((i) => i - 1);
  };

  const slideComponents = [
    <Slide01_WhatsNew
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      hidePreviousButton
      key="slide-01"
    />,
    <Slide02_SuccessfulSwaps
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-02"
    />,
    <Slide03_WhatIfICancel
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-03"
    />,
    <Slide04_LearnMore
      handleContinue={handleContinue}
      handlePrevious={handlePrevious}
      key="slide-04"
    />,
  ];

  return (
    <Modal
      open={open}
      onClose={(_event, reason) => {
        if (reason !== "backdropClick") handleClose();
      }}
      sx={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
      disableAutoFocus
      closeAfterTransition
    >
      {slideComponents[currentSlideIndex]}
    </Modal>
  );
}
