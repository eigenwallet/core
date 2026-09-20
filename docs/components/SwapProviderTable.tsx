"use client";

import { useState, useEffect } from "react";

export default function SwapMakerTable() {
  function satsToBtc(sats) {
    return sats / 100000000;
  }

  async function getMakers() {
    const response = await fetch("https://api.unstoppableswap.net/api/list");
    const data = await response.json();
    return data;
  }

  const [makers, setMakers] = useState([]);

  useEffect(() => {
    getMakers().then((data) => {
      setMakers(data);
    });
  }, []);

  return (
    <div
      style={{
        overflowX: "scroll",
      }}
    >
      <table>
        <thead>
          <tr>
            <th>Network</th>
            <th>Multiaddress</th>
            <th>Peer ID</th>
            <th>Minimum Amount</th>
            <th>Maximum Amount</th>
            <th>Exchange Rate</th>
          </tr>
        </thead>
        <tbody>
          {makers.map((maker) => (
            <tr key={maker.peerId}>
              <td>{maker.testnet ? "Testnet" : "Mainnet"}</td>
              <td>{maker.multiAddr}</td>
              <td>{maker.peerId}</td>
              <td>{satsToBtc(maker.minSwapAmount)} BTC</td>
              <td>{satsToBtc(maker.maxSwapAmount)} BTC</td>
              <td>{satsToBtc(maker.price)} XMR/BTC</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
