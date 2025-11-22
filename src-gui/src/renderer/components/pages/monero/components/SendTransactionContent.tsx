import {
  Button,
  Box,
  DialogActions,
  DialogContent,
  DialogTitle,
  Typography,
} from "@mui/material";
import { useState } from "react";
import {
  xmrToPiconeros,
  btcToSats,
} from "../../../../../utils/conversionUtils";
import SendAmountInput from "./SendAmountInput";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import BitcoinAddressTextField from "renderer/components/inputs/BitcoinAddressTextField";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { sendMoneroTransaction, withdrawBtc } from "renderer/rpc";
import { useAppSelector } from "store/hooks";
import { SendMoneroResponse, WithdrawBtcResponse } from "models/tauriModel";
import {
  isContextWithMoneroWallet,
  isContextWithBitcoinWallet,
} from "models/tauriModelExt";

interface SendTransactionContentProps {
  unlocked_balance: number;
  wallet: "monero" | "bitcoin";
  onClose: () => void;
  onSuccess: (response: SendMoneroResponse | WithdrawBtcResponse) => void;
}

export default function SendTransactionContent({
  unlocked_balance,
  wallet,
  onSuccess,
  onClose,
}: SendTransactionContentProps) {
  const walletCurrency = wallet === "monero" ? "XMR" : "BTC";
  const walletPrecision = wallet === "monero" ? 3 : 5;
  const isContextWithWallet =
    wallet === "monero"
      ? isContextWithMoneroWallet
      : isContextWithBitcoinWallet;
  const AddressTextField =
    wallet === "monero" ? MoneroAddressTextField : BitcoinAddressTextField;

  const [sendAddress, setSendAddress] = useState("");
  const [sendAmount, setSendAmount] = useState("");
  const [previousAmount, setPreviousAmount] = useState("");
  const [enableSend, setEnableSend] = useState(false);
  const [currency, setCurrency] = useState(walletCurrency);
  const [isMaxSelected, setIsMaxSelected] = useState(false);
  const [isSending, setIsSending] = useState(false);

  const showFiatRate = useAppSelector(
    (state) => state.settings.fetchFiatPrices,
  );
  const fiatCurrency = useAppSelector((state) => state.settings.fiatCurrency);
  const fiatPrice = useAppSelector((state) =>
    wallet === "monero" ? state.rates.xmrPrice : state.rates.btcPrice,
  );

  const handleCurrencyChange = (newCurrency: string) => {
    if (!showFiatRate || !fiatPrice || isMaxSelected || isSending) {
      return;
    }

    let amount = 0;
    if (sendAmount !== "") {
      amount =
        newCurrency === walletCurrency
          ? parseFloat(sendAmount) / fiatPrice
          : parseFloat(sendAmount) * fiatPrice;
    }
    setSendAmount(
      amount.toFixed(newCurrency === walletCurrency ? walletPrecision : 2),
    );
    setCurrency(newCurrency);
  };

  const handleMaxToggled = () => {
    if (isSending) return;
    if (isMaxSelected) {
      // Disable MAX mode - restore previous amount
      setIsMaxSelected(false);
      setSendAmount(previousAmount);
    } else {
      // Enable MAX mode - save current amount first
      setPreviousAmount(sendAmount);
      setIsMaxSelected(true);
      setSendAmount("<MAX>");
    }
  };

  const handleAmountChange = (newAmount: string) => {
    if (isSending) return;
    if (newAmount !== "<MAX>") {
      setIsMaxSelected(false);
    }
    setSendAmount(newAmount);
  };

  const handleAddressChange = (newAddress: string) => {
    if (isSending) return;
    setSendAddress(newAddress);
  };

  const walletAmount =
    currency === walletCurrency
      ? parseFloat(sendAmount)
      : fiatPrice !== null
        ? parseFloat(sendAmount) / fiatPrice
        : null;

  const handleSend = async () => {
    if (!sendAddress) {
      throw new Error("Address is required");
    }

    if (isMaxSelected) {
      if (wallet === "monero")
        return sendMoneroTransaction({
          address: sendAddress,
          amount: { type: "Sweep" },
        });
      else return withdrawBtc(sendAddress, undefined);
    } else {
      if (!sendAmount || sendAmount === "<MAX>" || walletAmount === null) {
        throw new Error("Amount is required");
      }

      if (wallet === "monero")
        return sendMoneroTransaction({
          address: sendAddress,
          amount: {
            type: "Specific",
            // Floor the amount to avoid rounding decimal amounts
            // The amount is in piconeros, so it NEEDS to be a whole number
            amount: Math.floor(xmrToPiconeros(walletAmount)),
          },
        });
      // likewise but in satoshis
      else return withdrawBtc(sendAddress, Math.floor(btcToSats(walletAmount)));
    }
  };

  const handleSendSuccess = (
    response: SendMoneroResponse | WithdrawBtcResponse,
  ) => {
    // Clear form after successful send
    handleClear();
    onSuccess(response);
  };

  const handleClear = () => {
    setSendAddress("");
    setSendAmount("");
    setPreviousAmount("");
    setIsMaxSelected(false);
  };

  const isSendDisabled =
    !enableSend || (!isMaxSelected && (!sendAmount || sendAmount === "<MAX>"));

  return (
    <>
      <DialogTitle>Send</DialogTitle>
      <DialogContent>
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <SendAmountInput
            unlocked_balance={unlocked_balance}
            amount={sendAmount}
            onAmountChange={handleAmountChange}
            onMaxToggled={handleMaxToggled}
            currency={currency}
            wallet={wallet}
            walletCurrency={walletCurrency}
            walletPrecision={walletPrecision}
            fiatCurrency={fiatCurrency}
            fiatPrice={fiatPrice}
            showFiatRate={showFiatRate}
            onCurrencyChange={handleCurrencyChange}
            disabled={isSending}
          />
          <AddressTextField
            address={sendAddress}
            onAddressChange={handleAddressChange}
            onAddressValidityChange={setEnableSend}
            label="Send to"
            fullWidth
            disabled={isSending}
          />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <PromiseInvokeButton
          onInvoke={handleSend}
          disabled={isSendDisabled}
          onSuccess={handleSendSuccess}
          onPendingChange={setIsSending}
          contextRequirement={isContextWithWallet}
        >
          Send
        </PromiseInvokeButton>
      </DialogActions>
    </>
  );
}
