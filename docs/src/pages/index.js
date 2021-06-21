import React from "react";
import clsx from "clsx";
import Layout from "@theme/Layout";
import Link from "@docusaurus/Link";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
// import useBaseUrl from "@docusaurus/useBaseUrl";
import styles from "./styles.module.css";
import Translate from "@docusaurus/Translate";
// import { translate } from "@docusaurus/Translate";

function Home() {
  const context = useDocusaurusContext();
  const { siteConfig = {} } = context;
  return (
    <Layout title="Homepage" description="Solana Documentation">
      <main>
        <section className={styles.features}>
          <div className="container">
            <div className="row cards__container">
              <div className={clsx("col col--4", styles.feature)}>
                <Link
                  className="navbar__link"
                  to="developing/programming-model/overview"
                >
                  <div className="card">
                    <div className="card__header">
                      <h3>
                        <Translate description="start-building">
                          ⛏ Start Building
                        </Translate>
                      </h3>
                    </div>
                    <div className="card__body">
                      <p>
                        <Translate description="get-started-building">
                          Get started building your decentralized app or
                          marketplace.
                        </Translate>
                      </p>
                    </div>
                  </div>
                </Link>
              </div>
              <div className={clsx("col col--4", styles.feature)}>
                <Link className="navbar__link" to="running-validator">
                  <div className="card">
                    <div className="card__header">
                      <h3>
                        <Translate description="run-validator">
                          🎛 Run a Validator Node
                        </Translate>
                      </h3>
                    </div>
                    <div className="card__body">
                      <p>
                        <Translate description="validate-transactions">
                          Validate transactions, secure the network, and earn
                          rewards.
                        </Translate>
                      </p>
                    </div>
                  </div>
                </Link>
              </div>
              <div className={clsx("col col--4", styles.feature)}>
                <Link
                  className="navbar__link"
                  to="https://spl.solana.com/token"
                >
                  <div className="card">
                    <div className="card__header">
                      <h3>
                        <Translate description="create-spl">
                          🏛 Create an SPL Token
                        </Translate>
                      </h3>
                    </div>
                    <div className="card__body">
                      <p>
                        <Translate description="erc-20">
                          Launch your own SPL Token, Solana's equivalent of
                          ERC-20.
                        </Translate>
                      </p>
                    </div>
                  </div>
                </Link>
              </div>
              <div className={clsx("col col--4", styles.feature)}>
                <Link className="navbar__link" to="integrations/exchange">
                  <div className="card">
                    <div className="card__header">
                      <h3>
                        <Translate description="integrate-exchange">
                          🏦 Integrate an Exchange
                        </Translate>
                      </h3>
                    </div>
                    <div className="card__body">
                      <p>
                        <Translate description="integration-guide">
                          Follow our extensive integration guide to ensure a
                          seamless user experience.
                        </Translate>
                      </p>
                    </div>
                  </div>
                </Link>
              </div>
              <div className={clsx("col col--4", styles.feature)}>
                <Link className="navbar__link" to="wallet-guide">
                  <div className="card">
                    <div className="card__header">
                      <h3>
                        <Translate description="manage-wallet">
                          📲 Manage a Wallet
                        </Translate>
                      </h3>
                    </div>
                    <div className="card__body">
                      <p>
                        <Translate description="wallet-options">
                          Create a wallet, check your balance, and learn about
                          wallet options.
                        </Translate>
                      </p>
                    </div>
                  </div>
                </Link>
              </div>
              <div className={clsx("col col--4", styles.feature)}>
                <Link className="navbar__link" to="cluster/overview">
                  <div className="card">
                    <div className="card__header">
                      <h3>
                        <Translate description="learn-how">
                          🤯 Learn How Solana Works
                        </Translate>
                      </h3>
                    </div>
                    <div className="card__body">
                      <p>
                        <Translate description="high-level">
                          Get a high-level understanding of Solana's
                          architecture.
                        </Translate>
                      </p>
                    </div>
                  </div>
                </Link>
              </div>
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}

export default Home;
