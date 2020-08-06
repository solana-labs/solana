import React from "react";
import clsx from "clsx";
import Layout from "@theme/Layout";
import Link from "@docusaurus/Link";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import useBaseUrl from "@docusaurus/useBaseUrl";
import styles from "./styles.module.css";

const features = [
  {
    title: <>⛏ Build an Application</>,
    imageUrl: "apps",
    description: <>Get started building your decentralized app or marketplace.</>,
  },
  {
    title: <>🎛 Run a Validator Node</>,
    imageUrl: "running-validator",
    description: <>Validate transactions, secure the network, and earn rewards.</>,
  },
  {
    title: <>🏛 Create an SPL Token</>,
    imageUrl: "https://spl.solana.com/token",
    description: (
      <>
        Launch your own SPL Token, Solana's equivalent of ERC-20.
      </>
    ),
  },
  {
    title: <>🏦 Integrate an Exchange</>,
    imageUrl: "integrations/exchange",
    description: (
      <>
        Follow our extensive integration guide to ensure a seamless user
        experience.
      </>
    ),
  },
  {
    title: <>📲 Manage a Wallet</>,
    imageUrl: "wallet-guide",
    description: (
      <>
        Create a wallet, check your balance, and learn about wallet options.
      </>
    ),
  },
  {
    title: <>🤯 Learn How Solana Works</>,
    imageUrl: "cluster/overview",
    description: (
      <>
        Get a high-level understanding of Solana's architecture.
      </>
    ),
  }, //
  // {
  //   title: <>Understand Our Economic Design</>,
  //   imageUrl: "implemented-proposals/ed_overview/ed_overview",
  //   description: (
  //     <>
  //       Solana's Economic Design provides a scalable blueprint for long term
  //       economic development and prosperity.
  //     </>
  //   ),
  // }
];

function Feature({ imageUrl, title, description }) {
  const imgUrl = useBaseUrl(imageUrl);
  return (
    <div className={clsx("col col--4", styles.feature)}>
      {imgUrl && (
        <Link className="navbar__link" to={imgUrl}>
          <div className="card">
            <div className="card__header">
              <h3>{title}</h3>
            </div>
            <div className="card__body">
              <p>{description}</p>
            </div>
          </div>
        </Link>
      )}
    </div>
  );
}

function Home() {
  const context = useDocusaurusContext();
  const { siteConfig = {} } = context;
  return (
    <Layout
      title="Homepage"
      description="Description will go into a meta tag in <head />"
    >
      {/* <header className={clsx("hero hero--primary", styles.heroBanner)}> */}
      {/* <div className="container">
          <h1 className="hero__title">{siteConfig.title}</h1>
          <p className="hero__subtitle">{siteConfig.tagline}</p> */}
      {/* <div className={styles.buttons}>
            <Link
              className={clsx(
                'button button--outline button--secondary button--lg',
                styles.getStarted,
              )}
              to={useBaseUrl('docs/')}>
              Get Started
            </Link>
          </div> */}
      {/* </div> */}
      {/* </header> */}
      <main>
        {features && features.length > 0 && (
          <section className={styles.features}>
            <div className="container">
              <div className="row cards__container">
                {features.map((props, idx) => (
                  <Feature key={idx} {...props} />
                ))}
              </div>
            </div>
          </section>
        )}
      </main>
    </Layout>
  );
}

export default Home;
