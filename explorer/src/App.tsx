import React from "react";

import { NetworkProvider } from "./providers/network";
import { TransactionsProvider } from "./providers/transactions";
import NetworkStatusButton from "./components/NetworkStatusButton";
import TransactionsCard from "./components/TransactionsCard";
import NetworkModal from "./components/NetworkModal";
import Logo from "./img/logos-solana/light-explorer-logo.svg";

function App() {
  const [showModal, setShowModal] = React.useState(false);
  return (
    <NetworkProvider>
      <NetworkModal show={showModal} onClose={() => setShowModal(false)} />
      <div className="main-content">
        <div className="header">
          <div className="container">
            <div className="header-body">
              <div className="row align-items-end">
                <div className="col">
                  <img src={Logo} width="250" alt="Solana Explorer" />
                </div>
                <div className="col-auto">
                  <NetworkStatusButton onClick={() => setShowModal(true)} />
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
      <Overlay show={showModal} onClick={() => setShowModal(false)} />
    </NetworkProvider>
  );
}

type OverlayProps = {
  show: boolean;
  onClick: () => void;
};

function Overlay({ show, onClick }: OverlayProps) {
  if (show)
    return <div className="modal-backdrop fade show" onClick={onClick}></div>;

  return <div className="fade"></div>;
}

export default App;
