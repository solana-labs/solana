import React from "react";
import clsx from "clsx";
import Layout from "@theme/Layout";
import Link from "@docusaurus/Link";
import useDocusaurusContext from "@docusaurus/useDocusaurusContext";
import useBaseUrl from "@docusaurus/useBaseUrl";
import styles from "./styles.module.css";

const features = [
  {
    title: <>Run a Validator</>,
    imageUrl: "running-validator",
    description: <>Learn how to start a validator on the Solana cluster.</>,
  },
  {
    title: <>Launch an Application</>,
    imageUrl: "apps",
    description: <>Build superfast applications with one API.</>,
  },
  {
    title: <>Participate in Tour de SOL</>,
    imageUrl: "tour-de-sol",
    description: (
      <>
        Participate in our incentivised testnet and earn rewards by finding
        bugs.
      </>
    ),
  },
  {
    title: <>Integrate the SOL token into your Exchange</>,
    imageUrl: "integrations/exchange",
    description: (
      <>
        Follow our extensive integration guide to ensure a seamless user
        experience.
      </>
    ),
  },
  {
    title: <>Create or Configure a Solana Wallet</>,
    imageUrl: "wallet-guide",
    description: (
      <>
        Whether you need to create a wallet, check the balance of your funds, or
        take a look at what's out there for housing SOL tokens, start here.
      </>
    ),
  },
  {
    title: <>Learn About Solana's Architecture</>,
    imageUrl: "cluster/overview",
    description: (
      <>
        Familiarize yourself with the high level architecture of a Solana
        cluster.
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
