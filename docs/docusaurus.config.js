const math = require("remark-math");
const katex = require("rehype-katex");
module.exports = {
  title: "Solana Docs",
  tagline:
    "Solana is an open source project implementing a new, high-performance, permissionless blockchain.",
  url: "https://docs.solana.com",
  baseUrl: "/",
  favicon: "img/favicon.ico",
  organizationName: "solana-labs", // Usually your GitHub org/user name.
  projectName: "solana", // Usually your repo name.
  onBrokenLinks: "throw",
  stylesheets: [
    {
      href: "/katex/katex.min.css",
      type: "text/css",
      integrity:
        "sha384-AfEj0r4/OFrOo5t7NnNe46zW/tFgW6x/bCJG8FqQCEo3+Aro6EYUG4+cU+KJWu/X",
      crossorigin: "anonymous",
    },
  ],
  i18n: {
    defaultLocale: "en",
    locales: ["en", "de", "es", "ru", "ar"],
    // localesNotBuilding: ["ko", "pt", "vi", "zh", "ja"],
    localeConfigs: {
      en: {
        label: "English",
      },
      ru: {
        label: "Русский",
      },
      es: {
        label: "Español",
      },
      de: {
        label: "Deutsch",
      },
      ar: {
        label: "العربية",
      },
      ko: {
        label: "한국어",
      },
    },
  },
  themeConfig: {
    navbar: {
      logo: {
        alt: "Solana Logo",
        src: "img/logo-horizontal.svg",
        srcDark: "img/logo-horizontal-dark.svg",
      },
      items: [
        {
          href: "https://spl.solana.com",
          label: "Program Library »",
          position: "left",
        },
        {
          to: "developing/programming-model/overview",
          label: "Develop",
          position: "left",
        },
        {
          to: "running-validator",
          label: "Validate",
          position: "left",
        },
        {
          to: "integrations/exchange",
          label: "Integrate",
          position: "left",
        },
        {
          to: "cluster/overview",
          label: "Learn",
          position: "left",
        },
        {
          type: "localeDropdown",
          position: "right",
        },
        {
          href: "https://discordapp.com/invite/pquxPsq",
          label: "Chat",
          position: "right",
        },
        {
          href: "https://github.com/solana-labs/solana",
          label: "GitHub",
          position: "right",
        },
      ],
    },
    algolia: {
      // This API key is "search-only" and safe to be published
      apiKey: "d58e0d68c875346d52645d68b13f3ac0",
      indexName: "solana",
      contextualSearch: true,
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Documentation",
          items: [
            {
              label: "Introduction",
              to: "introduction",
            },
            {
              label: "Developing",
              to: "developing/programming-model/overview",
            },
            {
              label: "Validators",
              to: "running-validator",
            },
            {
              label: "Command Line",
              to: "cli",
            },
            {
              label: "Architecture",
              to: "cluster/overview",
            },
          ],
        },
        {
          title: "Community",
          items: [
            {
              label: "GitHub »",
              href: "https://github.com/solana-labs/solana",
            },
            {
              label: "Discord »",
              href: "https://discordapp.com/invite/pquxPsq",
            },
            {
              label: "Twitter »",
              href: "https://twitter.com/solana",
            },
            {
              label: "Forums »",
              href: "https://forums.solana.com",
            },
          ],
        },
        {
          title: "Resources",
          items: [
            {
              label: "Proposals",
              to: "proposals",
            },
            {
              href: "https://spl.solana.com",
              label: "Program Library »",
            },
            {
              label: "Integrations",
              to: "integrations/exchange",
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Solana Foundation`,
    },
  },
  presets: [
    [
      "@docusaurus/preset-classic",
      {
        docs: {
          path: "src",
          routeBasePath: "/",
          sidebarPath: require.resolve("./sidebars.js"),
          remarkPlugins: [math],
          rehypePlugins: [katex],
        },
        theme: {
          customCss: require.resolve("./src/css/custom.css"),
        },
      },
    ],
  ],
};
