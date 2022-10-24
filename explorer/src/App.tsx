import React from "react";
import { Switch, Route, Redirect } from "react-router-dom";

import { ClusterModal } from "components/ClusterModal";
import { MessageBanner } from "components/MessageBanner";
import { Navbar } from "components/Navbar";
import { ClusterStatusBanner } from "components/ClusterStatusButton";
import { SearchBar } from "components/SearchBar";

import { AccountDetailsPage } from "pages/address/[...address]";
import { TransactionInspectorPage } from "pages/tx/inspector";
import { ClusterStatsPage } from "pages/index";
import { SupplyPage } from "pages/supply";
import { TransactionDetailsPage } from "pages/tx/[signature]";
import { BlockDetailsPage } from "pages/block/[...block]";
import { EpochDetailsPage } from "pages/epoch/[epoch]";

const ADDRESS_ALIASES = ["account", "accounts", "addresses"];
const TX_ALIASES = ["txs", "txn", "txns", "transaction", "transactions"];

function App() {
  return (
    <>
      <ClusterModal />
      <div className="main-content pb-4">
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
            path={TX_ALIASES.map((tx) => `/${tx}/:signature`)}
            render={({ match, location }) => {
              let pathname = `/tx/${match.params.signature}`;
              return <Redirect to={{ ...location, pathname }} />;
            }}
          />
          <Route
            exact
            path={["/tx/inspector", "/tx/:signature/inspect"]}
            render={({ match }) => {
              const signature =
                "signature" in match.params
                  ? match.params.signature
                  : undefined;
              return <TransactionInspectorPage signature={signature} />;
            }}
          />
          <Route
            exact
            path={"/tx/:signature"}
            render={({ match }) => (
              <TransactionDetailsPage signature={match.params.signature} />
            )}
          />
          <Route
            exact
            path={"/epoch/:id"}
            render={({ match }) => <EpochDetailsPage epoch={match.params.id} />}
          />
          <Route
            exact
            path={["/block/:id", "/block/:id/:tab"]}
            render={({ match }) => {
              const tab = "tab" in match.params ? match.params.tab : undefined;
              return <BlockDetailsPage slot={match.params.id} tab={tab} />;
            }}
          />
          <Route
            exact
            path={[
              ...ADDRESS_ALIASES.map((path) => `/${path}/:address`),
              ...ADDRESS_ALIASES.map((path) => `/${path}/:address/:tab`),
            ]}
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
            render={({ match }) => {
              const tab = "tab" in match.params ? match.params.tab : undefined;
              return (
                <AccountDetailsPage address={match.params.address} tab={tab} />
              );
            }}
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
