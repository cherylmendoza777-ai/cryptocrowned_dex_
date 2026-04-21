import { useState } from "react";

export default function App() {
  const [account, setAccount] = useState(null);
  const [quote, setQuote] = useState(null);

  async function connectWallet() {
    if (!window.ethereum) {
      alert("Install MetaMask");
      return;
    }

    const accounts = await window.ethereum.request({
      method: "eth_requestAccounts",
    });

    setAccount(accounts[0]);
  }

  async function getQuote() {
    const res = await fetch(
      `/api/quote?sellToken=ETH&buyToken=DAI&sellAmount=1000000000000000&takerAddress=${account}`
    );

    const data = await res.json();
    setQuote(data);
  }

  async function executeSwap() {
    if (!account) {
      alert("Connect wallet first");
      return;
    }

    const res = await fetch(
      `/api/quote?sellToken=ETH&buyToken=DAI&sellAmount=1000000000000000&takerAddress=${account}`
    );

    const data = await res.json();

    if (!data.to) {
      alert("Swap failed");
      return;
    }

    await window.ethereum.request({
      method: "eth_sendTransaction",
      params: [{
        from: account,
        to: data.to,
        data: data.data,
        value: data.value || "0x0",
      }]
    });
  }

  return (
    <div style={{ background: "#0b0f14", color: "white", minHeight: "100vh", padding: 30 }}>
      <h1 style={{ color: "#f5c542" }}>👑 Crowned DEX</h1>

      <button onClick={connectWallet}>
        {account ? account : "Connect Wallet"}
      </button>

      <br /><br />

      <button onClick={getQuote}>Get Quote</button>
      <button onClick={executeSwap}>Execute Swap</button>

      {quote && (
        <pre>{JSON.stringify(quote, null, 2)}</pre>
      )}
    </div>
  );
}
