import { Box, Button, Typography } from "@mui/material";
import { openUrl } from "@tauri-apps/plugin-opener";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";

const GITHUB_ISSUE_URL =
  "https://github.com/eigenwallet/core/issues/new/choose";
const MATRIX_ROOM_URL = "https://matrix.to/#/#unstoppableswap:matrix.org";
export const DISCORD_URL = "https://discord.gg/aqSyyJ35UW";

export default function ContactInfoBox() {
  return (
    <InfoBox
      title="Get in touch"
      mainContent={
        <Typography variant="subtitle2">
          If you need help or just want to reach out to the contributors of this
          project you can open a GitHub issue, join our Matrix room or Discord
        </Typography>
      }
      additionalContent={
        <Box sx={{ display: "flex", gap: 1 }}>
          <Button variant="outlined" onClick={() => openUrl(GITHUB_ISSUE_URL)}>
            Open GitHub issue
          </Button>
          <Button variant="outlined" onClick={() => openUrl(MATRIX_ROOM_URL)}>
            Join Matrix room
          </Button>
          <Button variant="outlined" onClick={() => openUrl(DISCORD_URL)}>
            Join Discord
          </Button>
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
