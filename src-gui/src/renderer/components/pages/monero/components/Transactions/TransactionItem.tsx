import { TransactionInfo } from "models/tauriModel";
import { useIsMobile } from "utils/useIsMobile";
import TransactionItemMobile from "./TransactionItem.mobile";
import TransactionItemDesktop from "./TransactionItem.desktop";

interface TransactionItemProps {
  transaction: TransactionInfo;
  onClick?: () => void;
}

export default function TransactionItem({ transaction, onClick }: TransactionItemProps) {
  const isMobile = useIsMobile();

  // Return mobile or desktop layout based on screen size
  if (isMobile) {
    return (
      <TransactionItemMobile
        transaction={transaction}
        onClick={onClick}
      />
    );
  }

  return (
    <TransactionItemDesktop
      transaction={transaction}
    />
  );
}
