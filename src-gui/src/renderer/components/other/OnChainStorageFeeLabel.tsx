import { Tooltip } from "@mui/material";
import HelpIcon from "@mui/icons-material/HelpOutline";

/// The label of the Apollo transaction fee, with a "?" that explains it.
export default function OnChainStorageFeeLabel() {
  return (
    <>
      On-chain data storage fee
      <Tooltip title="Fee for the extra Bitcoin transaction that stores an encrypted backup of this swap on the Bitcoin chain. It lets you recover the swap from your seed if you lose your local swap data.">
        <HelpIcon
          sx={{ fontSize: "1em", ml: 0.5, verticalAlign: "text-bottom" }}
        />
      </Tooltip>
    </>
  );
}
