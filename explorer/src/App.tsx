import React from "react";

import { ClusterProvider } from "./providers/cluster";
import { TransactionsProvider } from "./providers/transactions";
import { AccountsProvider } from "./providers/accounts";
import ClusterStatusButton from "./components/ClusterStatusButton";
import AccountsCard from "./components/AccountsCard";
import TransactionsCard from "./components/TransactionsCard";
import ClusterModal from "./components/ClusterModal";
import Logo from "./img/logos-solana/light-explorer-logo.svg";

function App() {
  const [showModal, setShowModal] = React.useState(false);
  return (
    <ClusterProvider>
      <ClusterModal show={showModal} onClose={() => setShowModal(false)} />
      <div className="main-content">
        <div className="header">
          <div className="container">
            <div className="header-body">
              <div className="row align-items-end">
                <div className="col">
                  <img src={Logo} width="250" alt="Solana Explorer" />
                </div>
                <div className="col-auto">
                  <ClusterStatusButton onClick={() => setShowModal(true)} />
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
          <div className="row">
            <div className="col-12">
              <AccountsProvider>
                <AccountsCard />
              </AccountsProvider>
            </div>
          </div>
        </div>
      </div>
      <Overlay show={showModal} onClick={() => setShowModal(false)} />
    </ClusterProvider>
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
