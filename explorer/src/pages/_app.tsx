import "styles/theme-dark.scss";
import React from "react";
import Head from "next/head";
import type { AppProps } from "next/app";
import * as Sentry from "@sentry/nextjs";

import { ClusterProvider } from "providers/cluster";
import { RichListProvider } from "providers/richList";
import { SupplyProvider } from "providers/supply";
import { TransactionsProvider } from "providers/transactions";
import { AccountsProvider } from "providers/accounts";
import { BlockProvider } from "providers/block";
import { EpochProvider } from "providers/epoch";
import { ScrollAnchorProvider } from "providers/scroll-anchor";
import { StatsProvider } from "providers/stats";
import { MintsProvider } from "providers/mints";

import { ClusterModal } from "components/ClusterModal";
import { MessageBanner } from "components/MessageBanner";
import { Navbar } from "components/Navbar";
import { ClusterStatusBanner } from "components/ClusterStatusButton";
import { SearchBar } from "components/SearchBar";

function ExplorerApp({ Component, pageProps }: AppProps) {
  React.useEffect(() => {
    if (process.env.NODE_ENV === "production") {
      Sentry.init({
        dsn: "https://5efdc15b4828434fbe949b5daed472be@o434108.ingest.sentry.io/5390542",
      });
    }

    // @ts-ignore
    import("bootstrap/dist/js/bootstrap.min.js");
  }, []);

  return (
    <>
      <Head>
        <title>Explorer | Solana</title>
      </Head>

      <ScrollAnchorProvider>
        <ClusterProvider>
          <StatsProvider>
            <SupplyProvider>
              <RichListProvider>
                <AccountsProvider>
                  <BlockProvider>
                    <EpochProvider>
                      <MintsProvider>
                        <TransactionsProvider>
                          <SharedLayout>
                            <Component {...pageProps} />
                          </SharedLayout>
                        </TransactionsProvider>
                      </MintsProvider>
                    </EpochProvider>
                  </BlockProvider>
                </AccountsProvider>
              </RichListProvider>
            </SupplyProvider>
          </StatsProvider>
        </ClusterProvider>
      </ScrollAnchorProvider>
    </>
  );
}

function SharedLayout({ children }: { children: React.ReactNode }) {
  return (
    <>
      <ClusterModal />

      <div className="main-content pb-4">
        <Navbar />
        <MessageBanner />
        <ClusterStatusBanner />
        <SearchBar />

        {children}
      </div>
    </>
  );
}

export default ExplorerApp;
