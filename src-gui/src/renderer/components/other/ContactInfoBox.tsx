import MatrixIcon from "../icons/MatrixIcon";
import { MenuBook } from "@mui/icons-material";
import DiscordIcon from "../icons/DiscordIcon";
import GitHubIcon from "@mui/icons-material/GitHub";
import LinkIconButton from "../icons/LinkIconButton";
import { Box, Tooltip } from "@mui/material";

export default function ContactInfoBox() {
  return (
    <Box
      sx={{
        display: "flex",
        justifyContent: "space-evenly",
      }}
    >
      <Tooltip title="Check out the GitHub repository">
        <span>
          <LinkIconButton url="https://github.com/eigenwallet/core">
            <GitHubIcon />
          </LinkIconButton>
        </span>
      </Tooltip>
      <Tooltip title="Join the Matrix room">
        <span>
          <LinkIconButton url="https://eigenwallet.org/matrix">
            <MatrixIcon />
          </LinkIconButton>
        </span>
      </Tooltip>
      <Tooltip title="Join the Discord server">
        <span>
          <LinkIconButton url="https://eigenwallet.org/discord">
            <DiscordIcon />
          </LinkIconButton>
        </span>
      </Tooltip>
      <Tooltip title="Read our official documentation">
        <span>
          <LinkIconButton url="https://docs.unstoppableswap.net">
            <MenuBook />
          </LinkIconButton>
        </span>
      </Tooltip>
    </Box>
  );
}
