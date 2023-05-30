import React from "react";
import Link from "@docusaurus/Link";
import styles from "./styles.module.css";
import Card from "../../components/Card";
import CardLayout from "../../layouts/CardLayout";

function GetStartedPage() {
  return (
    <CardLayout
      sidebarKey="developerSidebar"
      title="Developer Quick Start Guides"
      description="Solana Documentation"
      path="/getstarted"
    >
      <section className={styles.features}>
        <div className="container">
          <section>
            <h1>Get started with Solana development</h1>

            <div className="container__spacer">
              <p>
                Build and deploy your first on chain Solana program directly in
                your browser.
              </p>

              <Link to="/getstarted/hello-world" className="button">
                Get Started
              </Link>
            </div>
          </section>

          <section className="">
            <div className="row cards__container">
            <Card
                to="/getstarted/overview"
                header={{
                  label: "Overview",
                  translateId: "getstarted-overview",
                }}
                body={{
                  label:
                    "Learn the basics of developing on the Solana blockchain.",
                  translateId: "getstarted-overview-body",
                }}
              />
              <Card
                to="/getstarted/hello-world"
                header={{
                  label: "Hello World in your Browser",
                  translateId: "getstarted-hello",
                }}
                body={{
                  label:
                    "Write and deploy your first Solana program directly in your browser. No install needed.",
                  translateId: "getstarted-hello-body",
                }}
              />

              <Card
                to="/getstarted/local"
                header={{
                  label: "Local development",
                  translateId: "getstarted-local",
                }}
                body={{
                  label:
                    "Setup your local development environment for writing on chain programs.",
                  translateId: "getstarted-c-body",
                }}
              />

              <Card
                to="/getstarted/rust"
                header={{
                  label: "Native Rust Program",
                  translateId: "getstarted-rust",
                }}
                body={{
                  label:
                    "Build and deploy an on chain Solana program with the Rust language.",
                  translateId: "getstarted-rust-body",
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
                  translateId: "getstarted-anchor",
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
                  translateId: "getstarted-seahorse",
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

export default GetStartedPage;
