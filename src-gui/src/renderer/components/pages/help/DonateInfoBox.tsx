import { Link, Typography } from "@mui/material";
import MoneroIcon from "renderer/components/icons/MoneroIcon";
import DepositAddressInfoBox from "renderer/components/pages/swap/swap/components/DepositAddressInfoBox";

const XMR_DONATE_ADDRESS =
  "87jS4C7ngk9EHdqFFuxGFgg8AyH63dRUoULshWDybFJaP75UA89qsutG5B1L1QTc4w228nsqsv8EjhL7bz8fB3611Mh98mg";

export default function DonateInfoBox() {
  return (
    <DepositAddressInfoBox
      title="Donate"
      address={XMR_DONATE_ADDRESS}
      icon={<MoneroIcon />}
      additionalContent={
        <Typography variant="subtitle2">
          If you want to support our effort event further, you can do so at this
          address.
        </Typography>
      }
    />
  );
}
