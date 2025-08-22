import { SendMoneroResponse, SendMoneroArgs } from "models/tauriModel";
import { useState } from "react";
import { useAppSelector } from "store/hooks";
import { sendMoneroTransaction } from "renderer/rpc";
import { xmrToPiconeros } from "./conversionUtils";

export function useCreateSendTransaction(onSuccess: (response: SendMoneroResponse) => void) {
    const [sendAddress, setSendAddress] = useState("");
    const [sendAmount, setSendAmount] = useState("");
    const [previousAmount, setPreviousAmount] = useState("");
    const [validAddress, setValidAddress] = useState(false);
    const [currency, setCurrency] = useState("XMR");
    const [isMaxSelected, setIsMaxSelected] = useState(false);
    const [isSending, setIsSending] = useState(false);
  
    const showFiatRate = useAppSelector(
      (state) => state.settings.fetchFiatPrices,
    );

    const xmrPrice = useAppSelector((state) => state.rates.xmrPrice);
  
    const handleCurrencyChange = (newCurrency: string) => {
      if (!showFiatRate || !xmrPrice || isMaxSelected || isSending) {
        return;
      }
  
      if (sendAmount === "" || parseFloat(sendAmount) === 0) {
        setSendAmount(newCurrency === "XMR" ? "0.000" : "0.00");
      } else {
        setSendAmount(
          newCurrency === "XMR"
            ? (parseFloat(sendAmount) / xmrPrice).toFixed(3)
            : (parseFloat(sendAmount) * xmrPrice).toFixed(2),
        );
      }
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
  
    const moneroAmount =
      currency === "XMR"
        ? parseFloat(sendAmount)
        : parseFloat(sendAmount) / xmrPrice;
  
    const handleSend = async () => {
      if (!sendAddress) {
        throw new Error("Address is required");
      }
  
      if (isMaxSelected) {
        return sendMoneroTransaction({
          address: sendAddress,
          amount: { type: "Sweep" },
        });
      } else {
        if (!sendAmount || sendAmount === "<MAX>") {
          throw new Error("Amount is required");
        }
  
        return sendMoneroTransaction({
          address: sendAddress,
          amount: {
            type: "Specific",
            // Floor the amount to avoid rounding decimal amounts
            // The amount is in piconeros, so it NEEDS to be a whole number
            amount: Math.floor(xmrToPiconeros(moneroAmount)),
          },
        });
      }
    };
  
    const handleSendSuccess = (response: SendMoneroResponse) => {
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
      !validAddress || (!isMaxSelected && (!sendAmount || sendAmount === "<MAX>"));

    
    
    return {
        sendAddress,
        handleAddressChange,
        sendAmount,
        handleAmountChange,
        isMaxSelected,
        handleMaxToggled,
        currency,
        handleCurrencyChange,
        isSending,
        isSendDisabled,
        setValidAddress,
        handleSend,
        setIsSending,
        handleSendSuccess,
        handleClear,
    }
}