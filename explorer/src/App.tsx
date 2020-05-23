import React from "react";
import { Link, Switch, Route, Redirect } from "react-router-dom";

import AccountsCard from "./components/AccountsCard";
import AccountDetails from "./components/AccountDetails";
import TransactionsCard from "./components/TransactionsCard";
import TransactionDetails from "./components/TransactionDetails";
import ClusterModal from "./components/ClusterModal";
import Logo from "./img/logos-solana/light-explorer-logo.svg";
import { TX_ALIASES } from "./providers/transactions";
import { ACCOUNT_ALIASES, ACCOUNT_ALIASES_PLURAL } from "./providers/accounts";
import TabbedPage from "components/TabbedPage";
import TopAccountsCard from "components/TopAccountsCard";
import SupplyCard from "components/SupplyCard";

function App() {
  return (
    <>
      <ClusterModal />
      <div className="main-content">
        <nav className="navbar navbar-expand-xl navbar-light">
          <div className="container">
            <div className="row align-items-end">
              <div className="col">
                <Link to={location => ({ ...location, pathname: "/" })}>
                  <img src={Logo} width="250" alt="Solana Explorer" />
                </Link>
              </div>
            </div>
          </div>
        </nav>

        <Switch>
          <Route exact path="/supply">
            <TabbedPage tab="Supply">
              <SupplyCard />
            </TabbedPage>
          </Route>
          <Route exact path="/accounts/top">
            <TabbedPage tab="Accounts">
              <TopAccountsCard />
            </TabbedPage>
          </Route>
          <Route
            exact
            path={TX_ALIASES.flatMap(tx => [tx, tx + "s"]).map(
              tx => `/${tx}/:signature`
            )}
            render={({ match }) => (
              <TransactionDetails signature={match.params.signature} />
            )}
          />
          <Route exact path={TX_ALIASES.map(tx => `/${tx}s`)}>
            <TabbedPage tab="Transactions">
              <TransactionsCard />
            </TabbedPage>
          </Route>
          <Route
            exact
            path={ACCOUNT_ALIASES.concat(ACCOUNT_ALIASES_PLURAL).map(
              account => `/${account}/:address`
            )}
            render={({ match }) => (
              <AccountDetails address={match.params.address} />
            )}
          />
          <Route exact path={ACCOUNT_ALIASES_PLURAL.map(alias => "/" + alias)}>
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
