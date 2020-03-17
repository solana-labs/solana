import React from "react";
import { NetworkProvider } from "./providers/network";
import { TransactionsProvider } from "./providers/transactions";
import NetworkStatusButton from "./components/NetworkStatusButton";
import TransactionsCard from "./components/TransactionsCard";

function App() {
  return (
    <NetworkProvider>
      <div className="main-content">
        <div className="header">
          <div className="container">
            <div className="header-body">
              <div className="row align-items-end">
                <div className="col">
                  <h6 className="header-pretitle">Beta</h6>
                  <h1 className="header-title">Solana Explorer</h1>
                </div>
                <div className="col-auto">
                  <NetworkStatusButton />
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className="container">
          <div className="row">
            <div className="col-12">
              <TransactionsProvider>
                <TransactionsCard />
              </TransactionsProvider>
            </div>
          </div>
        </div>
      </div>
    </NetworkProvider>
  );
}

export default App;
