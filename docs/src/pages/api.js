import React from "react";
import Link from "@docusaurus/Link";
import styles from "./styles.module.css";
import Card from "../../components/Card";
import CardLayout from "../../layouts/CardLayout";

function APIPage() {
  return (
    <CardLayout
      sidebarKey="apiSidebar"
      title="JSON RPC API"
      description="Solana Documentation"
      path="/api"
    >
      <section className={styles.features}>
        <div className="container">
          <section>
            <h1>JSON RPC API</h1>

            <div className="container__spacer">
              <p>
                Interact with Solana nodes directly with the JSON RPC API via
                the HTTP and Websocket methods.
              </p>

              <Link to="/api/http">
                <a className="button">Explore the API</a>
              </Link>
            </div>
          </section>

          <section className="">
            <h2>Explore the JSON RPC Methods</h2>

            <div className="row cards__container">
              <Card
                to="api/http"
                header={{
                  label: "HTTP Methods",
                  translateId: "api-methods-http",
                }}
                body={{
                  label:
                    "Make direct requests to a Solana node via HTTP using the JSON RPC standard.",
                  translateId: "learn-methods-http",
                }}
              />

              <Card
                to="api/websocket"
                header={{
                  label: "Websocket Methods",
                  translateId: "api-methods-websocket",
                }}
                body={{
                  label:
                    "Monitor on-chain Solana data and events via a RPC PubSub Websocket connection.",
                  translateId: "learn-methods-websocket",
                }}
              />

              <Card
                to="https://github.com/solana-labs/solana-web3.js"
                header={{
                  label: "Web3.js",
                  translateId: "api-web3.js",
                }}
                body={{
                  label:
                    "Use the @solana/web3.js library to interact with a Solana node inside a JavaScript application.",
                  translateId: "learn-web3.js",
                }}
              />
            </div>
          </section>
        </div>
      </section>
    </CardLayout>
  );
}

export default APIPage;
