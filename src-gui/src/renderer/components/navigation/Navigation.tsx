import { useIsMobile } from "../../../utils/useIsMobile";
import MobileBottomNavigation from "./BottomNavigation";
import DesktopNavigation from "./DesktopNavigation";

export const drawerWidth = "240px";

export default function Navigation() {
  const isMobile = useIsMobile();

  return isMobile ? <MobileBottomNavigation /> : <DesktopNavigation />;
}
