import React from "react";
import Layout from "@theme/Layout";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import styles from "./styles.module.css";
import Card from "../../components/Card";

function Home() {
  const context = useDocusaurusContext();
  const { siteConfig = {} } = context;

  return (
    <Layout title="Homepage" description="Solana Documentation">
      <main>
        <section className={styles.features}>
          <div className="container">
            <div className="row cards__container">
              <Card
                to="developers"
                header={{
                  label: "⛏ Start Developing",
                  translateId: "start-building",
                }}
                body={{
                  label:
                    "Get started building your decentralized app or marketplace with Solana.",
                  translateId: "get-started-building",
                }}
              />

              <Card
                to="running-validator"
                header={{
                  label: "🎛 Run a Validator Node",
                  translateId: "run-validator",
                }}
                body={{
                  label:
                    "Validate transactions, secure the network, and earn rewards.",
                  translateId: "validate-transactions",
                }}
              />

              <Card
                to="https://spl.solana.com/token"
                header={{
                  label: "🏛 Create an SPL Token",
                  translateId: "create-spl",
                }}
                body={{
                  label:
                    "Launch your own SPL Token, Solana's equivalent of ERC-20.",
                  translateId: "erc-20",
                }}
              />

              <Card
                to="integrations/exchange"
                header={{
                  label: "🏦 Integrate an Exchange",
                  translateId: "integrate-exchange",
                }}
                body={{
                  label:
                    "Follow our extensive integration guide to ensure a seamless user experience.",
                  translateId: "integration-guide",
                }}
              />

              <Card
                to="wallet-guide"
                header={{
                  label: "📲 Manage a Wallet",
                  translateId: "manage-wallet",
                }}
                body={{
                  label:
                    "Create a wallet, check your balance, and learn about wallet options.",
                  translateId: "wallet-options",
                }}
              />

              <Card
                to="introduction"
                header={{
                  label: "🤯 Learn How Solana Works",
                  translateId: "learn-how",
                }}
                body={{
                  label:
                    "Get a high-level understanding of Solana's architecture.",
                  translateId: "high-level",
                }}
              />
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}

export default Home;
