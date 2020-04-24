import React from "react";
import ReactDOM from "react-dom";
import { BrowserRouter as Router } from "react-router-dom";
import "./scss/theme.scss";
import App from "./App";
import * as serviceWorker from "./serviceWorker";
import { ClusterProvider } from "./providers/cluster";
import { TransactionsProvider } from "./providers/transactions";
import { AccountsProvider } from "./providers/accounts";
import { TabProvider } from "./providers/tab";

ReactDOM.render(
  <Router>
    <TabProvider>
      <ClusterProvider>
        <AccountsProvider>
          <TransactionsProvider>
            <App />
          </TransactionsProvider>
        </AccountsProvider>
      </ClusterProvider>
    </TabProvider>
  </Router>,
  document.getElementById("root")
);

// If you want your app to work offline and load faster, you can change
// unregister() to register() below. Note this comes with some pitfalls.
// Learn more about service workers: https://bit.ly/CRA-PWA
serviceWorker.unregister();
