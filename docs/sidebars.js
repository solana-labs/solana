module.exports = {
  introductionSidebar: [
    {
      type: "category",
      collapsed: false,
      label: "Introduction to Solana",
      items: [
        {
          type: "doc",
          id: "introduction",
          label: "What is Solana?",
        },
        // This will be the future home for the economics overview page
        // {
        //   type: "doc",
        //   id: "economics_overview",
        //   label: "How do the economics work?",
        // },
        {
          type: "doc",
          id: "history",
          label: "History of Solana",
        },
      ],
    },
    {
      type: "category",
      collapsed: false,
      label: "Getting started with Solana",
      items: [
        {
          type: "doc",
          id: "wallet-guide",
          label: "Wallets",
        },
        // This will be the future home of the `staking` page, with the introductory info on what staking on Solana looks like
        // {
        //   type: "doc",
        //   id: "staking",
        //   label: "Staking",
        // },
      ],
    },
    {
      type: "category",
      collapsed: false,
      label: "Dive into Solana",
      items: [
        "terminology",
        {
          type: "link",
          href: "/developers",
          label: "Developers",
        },
        {
          type: "ref",
          label: "Validators",
          id: "running-validator",
        },
        {
          type: "ref",
          label: "Command Line",
          id: "cli",
        },
        {
          type: "ref",
          label: "Economics",
          id: "economics_overview",
        },
        {
          type: "ref",
          label: "Proposals",
          id: "proposals",
        },
      ],
    },
  ],
  developerSidebar: [
    {
      type: "link",
      href: "/developers",
      label: "Overview",
    },
    {
      type: "category",
      label: "Getting Started",
      items: ["developing/intro/programs", "developing/intro/rent"],
    },
    {
      type: "category",
      label: "Core Concepts",
      // collapsed: false,
      items: [
        "developing/programming-model/transactions",
        "developing/programming-model/accounts",
        "developing/programming-model/calling-between-programs",
        "developing/programming-model/runtime",
      ],
    },
    {
      type: "category",
      label: "Clients",
      items: [
        "developing/clients/jsonrpc-api",
        "developing/clients/javascript-api",
        "developing/clients/javascript-reference",
        "developing/clients/rust-api",
      ],
    },
    {
      type: "category",
      label: "Writing Programs",
      items: [
        "developing/on-chain-programs/overview",
        "developing/on-chain-programs/developing-rust",
        "developing/on-chain-programs/developing-c",
        {
          type: "doc",
          label: "Deploying",
          id: "developing/on-chain-programs/deploying",
        },
        {
          type: "doc",
          label: "Debugging",
          id: "developing/on-chain-programs/debugging",
        },
        "developing/on-chain-programs/examples",
        "developing/on-chain-programs/faq",
      ],
    },
    {
      type: "category",
      label: "Native Programs",
      items: [
        {
          type: "doc",
          label: "Overview",
          id: "developing/runtime-facilities/programs",
        },
        "developing/runtime-facilities/sysvars",
      ],
    },
    {
      type: "category",
      label: "Local Development",
      collapsed: false,
      items: ["developing/test-validator"],
    },
    "developing/backwards-compatibility",
  ],
  validatorsSidebar: [
    "running-validator",
    {
      type: "category",
      label: "Getting Started",
      collapsed: false,
      items: ["running-validator/validator-reqs"],
    },
    {
      type: "category",
      label: "Voting Setup",
      collapsed: false,
      items: [
        "running-validator/validator-start",
        "running-validator/vote-accounts",
        "running-validator/validator-stake",
        "running-validator/validator-monitor",
        "running-validator/validator-info",
        "running-validator/validator-failover",
        "running-validator/validator-troubleshoot",
      ],
    },
    {
      type: "category",
      label: "Geyser",
      collapsed: false,
      items: ["developing/plugins/geyser-plugins"],
    },
  ],
  cliSidebar: [
    "cli",
    "cli/install-solana-cli-tools",
    "cli/install-solana-cli-tools",
    {
      type: "category",
      label: "Command-line Wallets",
      items: [
        "wallet-guide/cli",
        "wallet-guide/paper-wallet",
        {
          type: "category",
          label: "Hardware Wallets",
          items: [
            "wallet-guide/hardware-wallets",
            "wallet-guide/hardware-wallets/ledger",
          ],
        },
        "wallet-guide/file-system-wallet",
        "wallet-guide/support",
      ],
    },
    "cli/conventions",
    "cli/choose-a-cluster",
    "cli/transfer-tokens",
    "cli/delegate-stake",
    "cli/deploy-a-program",
    "offline-signing",
    "offline-signing/durable-nonce",
    "cli/usage",
  ],
  architectureSidebar: [
    {
      type: "doc",
      label: "What is a Solana Cluster?",
      id: "cluster/overview",
    },
    {
      type: "category",
      label: "Clusters",
      collapsed: false,
      items: [
        "clusters",
        {
          type: "doc",
          label: "RPC Endpoints",
          id: "cluster/rpc-endpoints",
        },
        "cluster/bench-tps",
        "cluster/performance-metrics",
      ],
    },
    {
      type: "category",
      label: "Consensus",
      collapsed: false,
      items: [
        "cluster/synchronization",
        "cluster/leader-rotation",
        "cluster/fork-generation",
        "cluster/managing-forks",
        "cluster/turbine-block-propagation",
        "cluster/commitments",
        "cluster/vote-signing",
        "cluster/stake-delegation-and-rewards",
      ],
    },
    {
      type: "category",
      label: "Validators",
      collapsed: false,
      items: [
        {
          type: "doc",
          label: "Overview",
          id: "validator/anatomy",
        },
        "validator/tpu",
        "validator/tvu",
        "validator/blockstore",
        "validator/gossip",
        "validator/runtime",
      ],
    },
  ],
  "Design Proposals": [
    "proposals",
    {
      type: "category",
      label: "Accepted Proposals",
      collapsed: true,
      items: [
        {
          type: "autogenerated",
          dirName: "proposals",
        },
      ],
    },
    {
      type: "category",
      label: "Implemented  Proposals",
      collapsed: true,
      items: [
        {
          type: "autogenerated",
          dirName: "implemented-proposals",
        },
      ],
    },
  ],
  stakingSidebar: ["staking", "staking/stake-accounts"],
  integratingSidebar: [
    "integrations/exchange",
    "integrations/retrying-transactions",
  ],
  economicsSidebar: [
    {
      type: "doc",
      id: "economics_overview",
      // label: "How do the economics work?",
    },
    {
      type: "category",
      label: "Inflation Design",
      items: [
        "inflation/terminology",
        "inflation/inflation_schedule",
        "inflation/adjusted_staking_yield",
      ],
    },
    "transaction_fees",
    "storage_rent_economics",
  ],
};
