/** @type {import('next').NextConfig} */
const nextConfig = {
	reactStrictMode: true,

	images: {
		domains: ['raw.githubusercontent.com'],
	},

	webpack: (config) => {
		config.resolve.fallback = { fs: false }
		return config
	},

	async rewrites() {
		return [
			{
				source: '/(accounts|accounts\/top)',
				destination: '/supply'
			},
			{
				source: '/(account|accounts|addresses)/:address',
				destination: '/address/:address'
			},
			{
				source: '/(txs|txn|txns|transaction|transactions)/:signature',
				destination: '/tx/:signature'
			}
		]
	}
}

module.exports = nextConfig
