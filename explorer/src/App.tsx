import React from "react";
import { Link, Switch, Route, Redirect } from "react-router-dom";

import AccountsCard from "./components/AccountsCard";
import TransactionsCard from "./components/TransactionsCard";
import TransactionDetails from "./components/TransactionDetails";
import ClusterModal from "./components/ClusterModal";
import AccountModal from "./components/AccountModal";
import Logo from "./img/logos-solana/light-explorer-logo.svg";
import { TX_ALIASES } from "./providers/transactions";
import { ACCOUNT_PATHS } from "./providers/accounts";
import TabbedPage from "components/TabbedPage";

function App() {
  return (
    <>
      <ClusterModal />
      <AccountModal />
      <div className="main-content">
        <nav className="navbar navbar-expand-xl navbar-light">
          <div className="container">
            <div className="row align-items-end">
              <div className="col">
                <Link to="/">
                  <img src={Logo} width="250" alt="Solana Explorer" />
                </Link>
              </div>
            </div>
          </div>
        </nav>

        <Switch>
          <Route
            exact
            path={TX_ALIASES.map(tx => `/${tx}/:signature`)}
            render={({ match }) => (
              <TransactionDetails signature={match.params.signature} />
            )}
          />
          <Route exact path={TX_ALIASES.map(tx => `/${tx}s`)}>
            <TabbedPage tab="Transactions">
              <TransactionsCard />
            </TabbedPage>
          </Route>
          <Route path={ACCOUNT_PATHS}>
            <TabbedPage tab="Accounts">
              <AccountsCard />
            </TabbedPage>
          </Route>
          <Route
            render={({ location }) => (
              <Redirect to={{ ...location, pathname: "/transactions" }} />
            )}
          ></Route>
        </Switch>
      </div>
    </>
  );
}

export default App;
