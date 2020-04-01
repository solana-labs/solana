import React from "react";

import { ClusterProvider } from "./providers/cluster";
import {
  TransactionsProvider,
  useTransactionsDispatch,
  useTransactions,
  ActionType
} from "./providers/transactions";
import { AccountsProvider } from "./providers/accounts";
import { BlocksProvider } from "./providers/blocks";
import ClusterStatusButton from "./components/ClusterStatusButton";
import AccountsCard from "./components/AccountsCard";
import TransactionsCard from "./components/TransactionsCard";
import ClusterModal from "./components/ClusterModal";
import TransactionModal from "./components/TransactionModal";
import Logo from "./img/logos-solana/light-explorer-logo.svg";

function App() {
  const [showClusterModal, setShowClusterModal] = React.useState(false);
  return (
    <ClusterProvider>
      <TransactionsProvider>
        <BlocksProvider>
          <ClusterModal
            show={showClusterModal}
            onClose={() => setShowClusterModal(false)}
          />
          <TransactionModal />
          <div className="main-content">
            <div className="header">
              <div className="container">
                <div className="header-body">
                  <div className="row align-items-end">
                    <div className="col">
                      <img src={Logo} width="250" alt="Solana Explorer" />
                    </div>
                    <div className="col-auto">
                      <ClusterStatusButton
                        onClick={() => setShowClusterModal(true)}
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div className="container">
              <div className="row">
                <div className="col-12">
                  <TransactionsCard />
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
          <Overlay
            show={showClusterModal}
            onClick={() => setShowClusterModal(false)}
          />
        </BlocksProvider>
      </TransactionsProvider>
    </ClusterProvider>
  );
}

type OverlayProps = {
  show: boolean;
  onClick: () => void;
};

function Overlay({ show, onClick }: OverlayProps) {
  const { selected } = useTransactions();
  const dispatch = useTransactionsDispatch();

  if (show || !!selected)
    return (
      <div
        className="modal-backdrop fade show"
        onClick={() => {
          onClick();
          dispatch({ type: ActionType.Deselect });
        }}
      ></div>
    );

  return <div className="fade"></div>;
}

export default App;
