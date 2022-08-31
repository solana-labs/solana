import React from "react";
import styles from "./../styles.module.css";
import Card from "../../../components/Card";
import CardLayout from "../../../layouts/CardLayout";

function DevelopersQuickstart() {
  return (
    <CardLayout
      sidebarKey="developerSidebar"
      title="Developer Quick Start Guides"
      description="Solana Documentation"
      path="/developing/quickstart"
    >
      <section className={styles.features}>
        <div className="container">
          <section className="">
            <h1>Solana Developer Quick Start Guides</h1>

            <div className="row cards__container">
              <Card
                to="/developing/quickstart/rust"
                header={{
                  label: "Rust",
                  translateId: "quickstart-rust",
                }}
                body={{
                  label:
                    "Setup, build, and deploy a Rust based on chain program.",
                  translateId: "quickstart-rust-body",
                }}
              />

              <Card
                to="/developing/quickstart/c"
                header={{
                  label: "C / C++",
                  translateId: "quickstart-c",
                }}
                body={{
                  label:
                    "Setup, build, and deploy a C/C++ based on chain program.",
                  translateId: "quickstart-c-body",
                }}
              />

              <Card
                to="/developing/quickstart/web3js"
                header={{
                  label: "Web3.js",
                  translateId: "quickstart-web3js",
                }}
                body={{
                  label:
                    "Quickly get up and running with the Solana web3.js library.",
                  translateId: "quickstart-web3js-body",
                }}
              />
            </div>
          </section>

          <section className="">
            <h2>Community Resources</h2>

            <div className="row cards__container">
              <Card
                externalIcon={true}
                to="https://www.anchor-lang.com/"
                header={{
                  label: "Anchor Framework",
                  translateId: "quickstart-anchor",
                }}
                body={{
                  label: "Rust based framework for writing Solana programs.",
                  translateId: "start-building",
                }}
              />

              <Card
                externalIcon={true}
                to="https://seahorse-lang.org/"
                header={{
                  label: "Seahorse Lang",
                  translateId: "quickstart-seahorse",
                }}
                body={{
                  label: "Write Anchor-compatible Solana programs in Python.",
                  translateId: "learn-hello-world",
                }}
              />

              <Card
                externalIcon={true}
                to="https://beta.solpg.io/"
                header={{
                  label: "Solana Playground",
                  translateId: "developer-examples",
                }}
                body={{
                  label:
                    "Quickly develop, deploy and test Solana programs from the browser.",
                  translateId: "learn-by-example",
                }}
              />
            </div>
          </section>
        </div>
      </section>
    </CardLayout>
  );
}

export default DevelopersQuickstart;
