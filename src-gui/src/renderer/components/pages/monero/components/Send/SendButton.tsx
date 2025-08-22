import { useIsMobile } from "utils/useIsMobile";
import SendButtonMobile from "./SendButton.mobile";
import SendButtonDesktop from "./SendButton.desktop";

export default function SendButton({
  balance,
  disabled,
}: {
  balance: {
    unlocked_balance: string;
  };
  disabled?: boolean;
}) {
  return useIsMobile() ? (
    <SendButtonMobile balance={balance} disabled={disabled} />
  ) : (
    <SendButtonDesktop balance={balance} disabled={disabled} />
  );
}
