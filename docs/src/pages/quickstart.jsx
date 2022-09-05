import React from "react";
import Link from "@docusaurus/Link";
import styles from "./styles.module.css";
import Card from "../../components/Card";
import CardLayout from "../../layouts/CardLayout";

function DevelopersQuickstart() {
  return (
    <CardLayout
      sidebarKey="developerSidebar"
      title="Developer Quick Start Guides"
      description="Solana Documentation"
      path="/quickstart"
    >
      <section className={styles.features}>
        <div className="container">
          <section>
            <h1>Solana Quickstart Guides</h1>

            <div className="container__spacer">
              <p>
                Build and deploy your first on chain Solana program directly in
                your browser.
              </p>

              <Link to="/quickstart/hello-world">
                <a className="button">Get Started</a>
              </Link>
            </div>
          </section>

          <section className="">
            <div className="row cards__container">
              <Card
                to="/quickstart/hello-world"
                header={{
                  label: "Hello World in your Browser",
                  translateId: "quickstart-hello",
                }}
                body={{
                  label:
                    "Write and deploy your first Solana program directly in your browser.",
                  translateId: "quickstart-hello-body",
                }}
              />

              <Card
                to="/quickstart/local"
                header={{
                  label: "Local development",
                  translateId: "quickstart-local",
                }}
                body={{
                  label:
                    "Setup your local development environment for writing programs.",
                  translateId: "quickstart-c-body",
                }}
              />

              <Card
                to="/quickstart/rust"
                header={{
                  label: "Native Rust Program",
                  translateId: "quickstart-rust",
                }}
                body={{
                  label:
                    "Build and deploy a native Rust based on chain program.",
                  translateId: "quickstart-rust-body",
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
