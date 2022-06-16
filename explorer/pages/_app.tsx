import 'src/styles/theme-dark.scss'
import React from 'react'
import Head from 'next/head'
import type { AppProps } from 'next/app'
import * as Sentry from '@sentry/nextjs'

import { ClusterProvider } from 'src/providers/cluster'
import { RichListProvider } from 'src/providers/richList'
import { SupplyProvider } from 'src/providers/supply'
import { TransactionsProvider } from 'src/providers/transactions'
import { AccountsProvider } from 'src/providers/accounts'
import { BlockProvider } from 'src/providers/block'
import { EpochProvider } from 'src/providers/epoch'
import { ScrollAnchorProvider } from 'src/providers/scroll-anchor'
import { StatsProvider } from 'src/providers/stats'
import { MintsProvider } from 'src/providers/mints'

import { ClusterModal } from 'src/components/ClusterModal'
import { MessageBanner } from 'src/components/MessageBanner'
import { Navbar } from 'src/components/Navbar'
import { ClusterStatusBanner } from 'src/components/ClusterStatusButton'
import { SearchBar } from 'src/components/SearchBar'

function ExplorerApp({ Component, pageProps }: AppProps) {
	React.useEffect(() => {
		if (process.env.NODE_ENV === 'production') {
			Sentry.init({
				dsn: 'https://5efdc15b4828434fbe949b5daed472be@o434108.ingest.sentry.io/5390542',
			})
		}

		// @ts-ignore
		import('bootstrap/dist/js/bootstrap.min.js')
	}, [])

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
	)
}

function SharedLayout({ children }: { children: React.ReactNode }) {
	return (
		<div className="main-content pb-4">
			<Navbar />
			<MessageBanner />
			<ClusterStatusBanner />
			<SearchBar />

			{children}
		</div>
	)
}

export default ExplorerApp
