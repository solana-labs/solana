import React from "react";
import ReactDOM from "react-dom";
import { BrowserRouter as Router } from "react-router-dom";
import "./scss/theme-dark.scss";
import App from "./App";
import * as serviceWorker from "./serviceWorker";
import { ClusterProvider } from "./providers/cluster";
import { RichListProvider } from "./providers/richList";
import { SupplyProvider } from "./providers/supply";
import { TransactionsProvider } from "./providers/transactions";
import { AccountsProvider } from "./providers/accounts";
import { StatsProvider } from "providers/stats";
import { MintsProvider } from "providers/mints";

ReactDOM.render(
  <Router>
    <ClusterProvider>
      <StatsProvider>
        <SupplyProvider>
          <RichListProvider>
            <AccountsProvider>
              <MintsProvider>
                <TransactionsProvider>
                  <App />
                </TransactionsProvider>
              </MintsProvider>
            </AccountsProvider>
          </RichListProvider>
        </SupplyProvider>
      </StatsProvider>
    </ClusterProvider>
  </Router>,
  document.getElementById("root")
);

// If you want your app to work offline and load faster, you can change
// unregister() to register() below. Note this comes with some pitfalls.
// Learn more about service workers: https://bit.ly/CRA-PWA
serviceWorker.unregister();
