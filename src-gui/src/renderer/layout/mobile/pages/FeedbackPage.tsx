import { Box, IconButton, Typography } from "@mui/material";
import { useNavigate } from "react-router-dom";
import { ChevronLeft } from "@mui/icons-material";
import FeedbackInfoBox from "renderer/components/pages/help/FeedbackInfoBox";
import ConversationsBox from "renderer/components/pages/help/ConversationsBox";
import ContactInfoBox from "renderer/components/pages/help/ContactInfoBox";

export default function FeedbackPage() {
  const navigate = useNavigate();
  return (
    <Box>
      <Box sx={{ px: 2, pt: 3, display: "flex", alignItems: "center", gap: 1, position: "sticky", top: 0, backgroundColor: "background.paper", zIndex: 1 }}>
        <IconButton onClick={() => navigate("/", { viewTransition: true })}>
          <ChevronLeft />
        </IconButton>
        <Typography variant="h5">Feedback</Typography>
      </Box>
      <Box sx={{ p: 2, display: "flex", flexDirection: "column", gap: 2 }}>
        <FeedbackInfoBox />
        <ConversationsBox />
        <ContactInfoBox />
      </Box>
    </Box>
  );
}