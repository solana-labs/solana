import React from "react";
import { Switch, Route, Redirect } from "react-router-dom";

import AccountDetails from "./components/AccountDetails";
import TransactionDetails from "./components/TransactionDetails";
import ClusterModal from "./components/ClusterModal";
import { TX_ALIASES } from "./providers/transactions";
import { ACCOUNT_ALIASES, ACCOUNT_ALIASES_PLURAL } from "./providers/accounts";
import TopAccountsCard from "components/TopAccountsCard";
import SupplyCard from "components/SupplyCard";
import StatsCard from "components/StatsCard";
import MessageBanner from "components/MessageBanner";
import Navbar from "components/Navbar";
import { ClusterStatusBanner } from "components/ClusterStatusButton";

function App() {
  return (
    <>
      <ClusterModal />
      <div className="main-content">
        <Navbar />
        <MessageBanner />
        <ClusterStatusBanner />
        <Switch>
          <Route exact path={["/supply", "/accounts", "accounts/top"]}>
            <div className="container mt-4">
              <SupplyCard />
              <TopAccountsCard />
            </div>
          </Route>
          <Route
            exact
            path={TX_ALIASES.flatMap((tx) => [tx, tx + "s"]).map(
              (tx) => `/${tx}/:signature`
            )}
            render={({ match }) => (
              <TransactionDetails signature={match.params.signature} />
            )}
          />
          <Route
            exact
            path={ACCOUNT_ALIASES.concat(ACCOUNT_ALIASES_PLURAL).map(
              (account) => `/${account}/:address`
            )}
            render={({ match }) => (
              <AccountDetails address={match.params.address} />
            )}
          />
          <Route exact path="/">
            <div className="container mt-4">
              <StatsCard />
            </div>
          </Route>
          <Route
            render={({ location }) => (
              <Redirect to={{ ...location, pathname: "/" }} />
            )}
          />
        </Switch>
      </div>
    </>
  );
}

export default App;
