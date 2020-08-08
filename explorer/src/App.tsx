import React from "react";
import { Switch, Route, Redirect } from "react-router-dom";

import { ClusterModal } from "components/ClusterModal";
import { TX_ALIASES } from "providers/transactions";
import { MessageBanner } from "components/MessageBanner";
import { Navbar } from "components/Navbar";
import { ClusterStatusBanner } from "components/ClusterStatusButton";
import { SearchBar } from "components/SearchBar";

import { AccountDetailsPage } from "pages/AccountDetailsPage";
import { ClusterStatsPage } from "pages/ClusterStatsPage";
import { SupplyPage } from "pages/SupplyPage";
import { TransactionDetailsPage } from "pages/TransactionDetailsPage";

const ACCOUNT_ALIASES = ["account", "accounts", "addresses"];

function App() {
  return (
    <>
      <ClusterModal />
      <div className="main-content">
        <Navbar />
        <MessageBanner />
        <ClusterStatusBanner />
        <SearchBar />
        <Switch>
          <Route exact path={["/supply", "/accounts", "accounts/top"]}>
            <SupplyPage />
          </Route>
          <Route
            exact
            path={TX_ALIASES.flatMap((tx) => [tx, tx + "s"]).map(
              (tx) => `/${tx}/:signature`
            )}
            render={({ match }) => (
              <TransactionDetailsPage signature={match.params.signature} />
            )}
          />
          <Route
            exact
            path={ACCOUNT_ALIASES.flatMap((account) => [
              `/${account}/:address`,
              `/${account}/:address/:tab`,
            ])}
            render={({ match, location }) => {
              let pathname = `/address/${match.params.address}`;
              if (match.params.tab) {
                pathname += `/${match.params.tab}`;
              }
              return <Redirect to={{ ...location, pathname }} />;
            }}
          />
          <Route
            exact
            path={["/address/:address", "/address/:address/:tab"]}
            render={({ match }) => (
              <AccountDetailsPage
                address={match.params.address}
                tab={match.params.tab}
              />
            )}
          />
          <Route exact path="/">
            <ClusterStatsPage />
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
