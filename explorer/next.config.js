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
}

module.exports = nextConfig
