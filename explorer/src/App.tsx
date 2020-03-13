import React from "react";
import { NetworkProvider } from "./providers/network";
import NetworkStatusButton from "./components/networkStatusButton";

function App() {
  return (
    <div className="main-content">
      <div className="header">
        <div className="container-fluid">
          <div className="header-body">
            <div className="row align-items-end">
              <div className="col">
                <h6 className="header-pretitle">Beta</h6>
                <h1 className="header-title">Solana Explorer</h1>
              </div>
              <div className="col-auto">
                <NetworkProvider>
                  <NetworkStatusButton />
                </NetworkProvider>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
