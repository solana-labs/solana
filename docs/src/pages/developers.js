import React from "react";
import Layout from "@theme/Layout";
// import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import styles from "./styles.module.css";
import Card from "../../components/Card";

function Developers() {
  // const { siteConfig = {} } = useDocusaurusContext();

  return (
    <Layout title="Developers" description="Solana Documentation">
      <main>
        <section className={styles.features}>
          <div className="container">
            <section className="">
              <h2>Learn Solana development</h2>

              <div className="row cards__container">
                <Card
                  to="developing/intro/programs"
                  header={{
                    label: "Getting Started",
                    translateId: "start-building",
                  }}
                  body={{
                    label:
                      "Discover what Solana programs are and how they work.",
                    translateId: "start-building",
                  }}
                />

                <Card
                  to="developing/programming-model/transactions"
                  header={{
                    label: "Transactions",
                    translateId: "start-building",
                  }}
                  body={{
                    label:
                      "Program execution begins with a transaction being submitted to the cluster.",
                    translateId: "start-building",
                  }}
                />

                <Card
                  to="developing/programming-model/accounts"
                  header={{
                    label: "Accounts",
                    translateId: "run-validator",
                  }}
                  body={{
                    label:
                      "Understand how programs store data or state on the Solana blockchain.",
                    translateId: "validate-transactions",
                  }}
                />
              </div>
            </section>

            <section className="">
              <h2>Learn through coding</h2>

              <div className="row cards__container">
                <Card
                  to="developing/on-chain-programs/overview"
                  header={{
                    label: "Building Programs",
                    translateId: "start-building",
                  }}
                  body={{
                    label:
                      "Start developing on-chain Solana programs with Rust or C/C++.",
                    translateId: "start-building",
                  }}
                />

                <Card
                  to="developing/on-chain-programs/examples#helloworld"
                  header={{
                    label: "Hello World Example",
                    translateId: "run-validator",
                  }}
                  body={{
                    label:
                      "Example of how to use code to interact with the Solana blockchain.",
                    translateId: "validate-transactions",
                  }}
                />

                <Card
                  to="developing/on-chain-programs/examples"
                  header={{
                    label: "Example Programs",
                    translateId: "start-building",
                  }}
                  body={{
                    label:
                      "Browse and clone working example programs from GitHub.",
                    translateId: "start-building",
                  }}
                />
              </div>
            </section>

            <section className="">
              <h2>Setup your local development</h2>

              <div className="row cards__container">
                <Card
                  to="developing/clients/jsonrpc-api"
                  header={{
                    label: "Essential Tools",
                    translateId: "start-building",
                  }}
                  body={{
                    label:
                      "Explore the essential developer tools for building and deploying Solana programs.",
                    translateId: "start-building",
                  }}
                />

                <Card
                  to="developing/test-validator"
                  header={{
                    label: "Solana Test Validator",
                    translateId: "run-validator",
                  }}
                  body={{
                    label:
                      "Quickly setup and run a self contained local Solana blockchain for faster development.",
                    translateId: "validate-transactions",
                  }}
                />

                <Card
                  to="developing/on-chain-programs/debugging"
                  header={{
                    label: "Debugging Programs",
                    translateId: "start-building",
                  }}
                  body={{
                    label:
                      "Understand using unit test, logging. and error handling programs.",
                    translateId: "start-building",
                  }}
                />
              </div>
            </section>
          </div>
        </section>
      </main>
    </Layout>
  );
}

export default Developers;
