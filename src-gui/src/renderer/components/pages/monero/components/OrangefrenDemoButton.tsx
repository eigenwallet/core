import { ScatterPlot as DemoIcon } from "@mui/icons-material";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { orangefrenDemoTrade } from "renderer/rpc";
import { isContextWithMoneroWallet } from "models/tauriModelExt";

// Component for Orangefren demo button
export default function OrangefrenDemoButton() {
  return (
    <PromiseInvokeButton
      onInvoke={orangefrenDemoTrade}
      onSuccess={(response) => console.log("Orangefren demo trade:", response)}
      startIcon={<DemoIcon />}
      variant="outlined"
      tooltipTitle="Demo: Start an Orangefren BTC->XMR trade"
      displayErrorSnackbar
      isChipButton
      contextRequirement={isContextWithMoneroWallet}
    >
      Orangefren Demo
    </PromiseInvokeButton>
  );
}

