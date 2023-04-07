module.exports = {
  apiSidebar: [
    {
      type: "link",
      href: "/api",
      label: "JSON RPC API",
    },
    {
      type: "doc",
      id: "api/http",
      label: "HTTP Methods",
    },
    {
      type: "doc",
      id: "api/websocket",
      label: "Websocket Methods",
    },
  ],
  apiHttpMethodsSidebar: [
    {
      type: "link",
      href: "/api",
      label: "JSON RPC API",
    },
    {
      type: "doc",
      id: "api/websocket",
      label: "Websocket Methods",
    },
    {
      type: "category",
      link: { type: "doc", id: "api/http" },
      label: "HTTP Methods",
      collapsed: false,
      items: [
        {
          type: "link",
          href: "#getaccountinfo",
          label: "getAccountInfo",
        },
        {
          type: "link",
          href: "#getbalance",
          label: "getBalance",
        },
        {
          type: "link",
          href: "#getblockheight",
          label: "getBlockHeight",
        },
        {
          type: "link",
          href: "#getblock",
          label: "getBlock",
        },
        {
          type: "link",
          href: "#getblockproduction",
          label: "getBlockProduction",
        },
        {
          type: "link",
          href: "#getblockcommitment",
          label: "getBlockCommitment",
        },
        {
          type: "link",
          href: "#getblocks",
          label: "getBlocks",
        },
        {
          type: "link",
          href: "#getblockswithlimit",
          label: "getBlocksWithLimit",
        },
        {
          type: "link",
          href: "#getblocktime",
          label: "getBlockTime",
        },
        {
          type: "link",
          href: "#getclusternodes",
          label: "getClusterNodes",
        },
        {
          type: "link",
          href: "#getepochinfo",
          label: "getEpochInfo",
        },
        {
          type: "link",
          href: "#getepochschedule",
          label: "getEpochSchedule",
        },
        {
          type: "link",
          href: "#getfeeformessage",
          label: "getFeeForMessage",
        },
        {
          type: "link",
          href: "#getfirstavailableblock",
          label: "getFirstAvailableBlock",
        },
        {
          type: "link",
          href: "#getgenesishash",
          label: "getGenesisHash",
        },
        {
          type: "link",
          href: "#gethealth",
          label: "getHealth",
        },
        {
          type: "link",
          href: "#gethighestsnapshotslot",
          label: "getHighestSnapshotSlot",
        },
        {
          type: "link",
          href: "#getidentity",
          label: "getIdentity",
        },
        {
          type: "link",
          href: "#getinflationgovernor",
          label: "getInflationGovernor",
        },
        {
          type: "link",
          href: "#getinflationrate",
          label: "getInflationRate",
        },
        {
          type: "link",
          href: "#getinflationreward",
          label: "getInflationReward",
        },
        {
          type: "link",
          href: "#getlargestaccounts",
          label: "getLargestAccounts",
        },
        {
          type: "link",
          href: "#getlatestblockhash",
          label: "getLatestBlockhash",
        },
        {
          type: "link",
          href: "#getleaderschedule",
          label: "getLeaderSchedule",
        },
        {
          type: "link",
          href: "#getmaxretransmitslot",
          label: "getMaxRetransmitSlot",
        },
        {
          type: "link",
          href: "#getmaxshredinsertslot",
          label: "getMaxShredInsertSlot",
        },
        {
          type: "link",
          href: "#getminimumbalanceforrentexemption",
          label: "getMinimumBalanceForRentExemption",
        },
        {
          type: "link",
          href: "#getmultipleaccounts",
          label: "getMultipleAccounts",
        },
        {
          type: "link",
          href: "#getprogramaccounts",
          label: "getProgramAccounts",
        },
        {
          type: "link",
          href: "#getrecentperformancesamples",
          label: "getRecentPerformanceSamples",
        },
        {
          type: "link",
          href: "#getrecentprioritizationfees",
          label: "getRecentPrioritizationFees",
        },
        {
          type: "link",
          href: "#getsignaturesforaddress",
          label: "getSignaturesForAddress",
        },
        {
          type: "link",
          href: "#getsignaturestatuses",
          label: "getSignatureStatuses",
        },
        {
          type: "link",
          href: "#getslot",
          label: "getSlot",
        },
        {
          type: "link",
          href: "#getslotleader",
          label: "getSlotLeader",
        },
        {
          type: "link",
          href: "#getslotleaders",
          label: "getSlotLeaders",
        },
        {
          type: "link",
          href: "#getstakeactivation",
          label: "getStakeActivation",
        },
        {
          type: "link",
          href: "#getstakeminimumdelegation",
          label: "getStakeMinimumDelegation",
        },
        {
          type: "link",
          href: "#getsupply",
          label: "getSupply",
        },
        {
          type: "link",
          href: "#gettokenaccountbalance",
          label: "getTokenAccountBalance",
        },
        {
          type: "link",
          href: "#gettokenaccountsbydelegate",
          label: "getTokenAccountsByDelegate",
        },
        {
          type: "link",
          href: "#gettokenaccountsbyowner",
          label: "getTokenAccountsByOwner",
        },
        {
          type: "link",
          href: "#gettokenlargestaccounts",
          label: "getTokenLargestAccounts",
        },
        {
          type: "link",
          href: "#gettokensupply",
          label: "getTokenSupply",
        },
        {
          type: "link",
          href: "#gettransaction",
          label: "getTransaction",
        },
        {
          type: "link",
          href: "#gettransactioncount",
          label: "getTransactionCount",
        },
        {
          type: "link",
          href: "#getversion",
          label: "getVersion",
        },
        {
          type: "link",
          href: "#getvoteaccounts",
          label: "getVoteAccounts",
        },
        {
          type: "link",
          href: "#isblockhashvalid",
          label: "isBlockhashValid",
        },
        {
          type: "link",
          href: "#minimumledgerslot",
          label: "minimumLedgerSlot",
        },
        {
          type: "link",
          href: "#requestairdrop",
          label: "requestAirdrop",
        },
        {
          type: "link",
          href: "#sendtransaction",
          label: "sendTransaction",
        },
        {
          type: "link",
          href: "#simulatetransaction",
          label: "simulateTransaction",
        },
      ],
    },
    // {
    //   type: "category",
    //   label: "Unstable Methods",
    //   collapsed: true,
    //   items: [
    //     {
    //       type: "link",
    //       href: "#blocksubscribe",
    //       label: "blockSubscribe",
    //     },
    //   ],
    // },
    {
      type: "category",
      label: "Deprecated Methods",
      collapsed: true,
      items: [
        {
          type: "link",
          href: "#getconfirmedblock",
          label: "getConfirmedBlock",
        },
        {
          type: "link",
          href: "#getconfirmedblocks",
          label: "getConfirmedBlocks",
        },
        {
          type: "link",
          href: "#getconfirmedblockswithlimit",
          label: "getConfirmedBlocksWithLimit",
        },
        {
          type: "link",
          href: "#getconfirmedsignaturesforaddress2",
          label: "getConfirmedSignaturesForAddress2",
        },
        {
          type: "link",
          href: "#getconfirmedtransaction",
          label: "getConfirmedTransaction",
        },
        {
          type: "link",
          href: "#getfeecalculatorforblockhash",
          label: "getFeeCalculatorForBlockhash",
        },
        {
          type: "link",
          href: "#getfeerategovernor",
          label: "getFeeRateGovernor",
        },
        {
          type: "link",
          href: "#getfees",
          label: "getFees",
        },
        {
          type: "link",
          href: "#getrecentblockhash",
          label: "getRecentBlockhash",
        },
        {
          type: "link",
          href: "#getsnapshotslot",
          label: "getSnapshotSlot",
        },
      ],
    },
  ],
  apiWebsocketMethodsSidebar: [
    {
      type: "link",
      href: "/api",
      label: "JSON RPC API",
    },
    {
      type: "doc",
      id: "api/http",
      label: "HTTP Methods",
    },
    {
      type: "category",
      link: { type: "doc", id: "api/websocket" },
      label: "Websocket Methods",
      collapsed: false,
      items: [
        {
          type: "link",
          href: "#accountsubscribe",
          label: "accountSubscribe",
        },
        {
          type: "link",
          href: "#accountunsubscribe",
          label: "accountUnsubscribe",
        },
        {
          type: "link",
          href: "#logssubscribe",
          label: "logsSubscribe",
        },
        {
          type: "link",
          href: "#logsunsubscribe",
          label: "logsUnsubscribe",
        },
        {
          type: "link",
          href: "#programsubscribe",
          label: "programSubscribe",
        },
        {
          type: "link",
          href: "#programunsubscribe",
          label: "programUnsubscribe",
        },
        {
          type: "link",
          href: "#signaturesubscribe",
          label: "signatureSubscribe",
        },
        {
          type: "link",
          href: "#signatureunsubscribe",
          label: "signatureUnsubscribe",
        },
        {
          type: "link",
          href: "#slotsubscribe",
          label: "slotSubscribe",
        },
        {
          type: "link",
          href: "#slotunsubscribe",
          label: "slotUnsubscribe",
        },
      ],
    },
    {
      type: "category",
      label: "Unstable Methods",
      collapsed: false,
      items: [
        {
          type: "link",
          href: "#blocksubscribe",
          label: "blockSubscribe",
        },
        {
          type: "link",
          href: "#blockunsubscribe",
          label: "blockUnsubscribe",
        },
        {
          type: "link",
          href: "#slotsupdatessubscribe",
          label: "slotsUpdatesSubscribe",
        },
        {
          type: "link",
          href: "#slotsupdatesunsubscribe",
          label: "slotsUpdatesUnsubscribe",
        },
        {
          type: "link",
          href: "#votesubscribe",
          label: "voteSubscribe",
        },
        {
          type: "link",
          href: "#voteunsubscribe",
          label: "voteUnsubscribe",
        },
      ],
    },
    // {
    //   type: "category",
    //   label: "Deprecated Methods",
    //   collapsed: true,
    //   items: [
    //     {
    //       type: "link",
    //       href: "#getconfirmedblock",
    //       label: "getConfirmedBlock",
    //     },
    //   ],
    // },
  ],
};
