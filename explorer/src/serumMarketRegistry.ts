import { Cluster } from "providers/cluster";

const MARKET_REGISTRY: { [key: string]: string } = {};

function set(keys: string[], name: string) {
  keys.forEach((key) => (MARKET_REGISTRY[key] = name));
}

set(
  [
    "7kgkDyW7dmyMeP8KFXzbcUZz1R2WHsovDZ7n3ihZuNDS",
    "6WpkeqE5oU1MUNPWvMDHhru1G5gjxMgAtib5wXuBSvgm",
    "DwpUjRHQuotE1LG2R68wZM3nwkv2fChHibcm7NzL8WGq",
    "7zyPwxjHMJsTdPe7Rd992oe1cVhrZcbkcH9qURzKV8wV",
    "4nHe9oNh7JJoJZ1HrktVghB19Cis4N848so7UCWXhF2t",
    "8gbnu8XUNmigCSKP43UXbtYYTUHJPRbctyB7Kj1QbTyZ",
    "8MboeurJ28fQj3n18jBrM7oQSu3coyFbQUpxWnabF3gc",
    "GFyDCG3EBVrAWiHKLf7zF2DLqMp89dLHUtsYKwFUe4AC",
  ],
  "Serum: MSRM / USDC"
);

set(
  [
    "H4snTKK9adiU15gP22ErfZYtro3aqR9BTMXiH3AwiUTQ",
    "3mrit8EKnsy9M8L7EQ24GjNeuwXssVGqDRLZXDarb9Wk",
    "98qQ1Dintci8xSUAHEJqQukSKnE9g1LQY2jxNwwgcQQu",
    "83Snk2SJTX8KKMPmi5UX9JYKxy2QWrn2jFC9zH6NmP7L",
    "CKjPS8ntVbN7YjGk4Goq24ScaA1qjNFFqQpVuzNkFRo4",
    "mKeCpjYQWzptDQn5J5XfSzyoNmsnH9W3RryzUWFe3G7",
    "9o8XKrbbbA8eXMe72Tsopkk8y7aFF2HQPDMoxdyX4S9b",
    "12rqwuEgBYiGhBrDJStCiqEtzQpTTiZbh7teNVLuYcFA",
  ],
  "Serum: MSRM / USDT"
);

set(
  [
    "CAgAeMD7quTdnr6RPa7JySQpjf3irAmefYNdTb6anemq",
    "5PMuDUdk7VFLSYXDo6wHsEfyfWAf1rG4rkdmrmnK4ZME",
    "DrGCgNJAwpihVrRCzd69Ys6k5ggu1qC2FQFjCESnj3Do",
    "6oVGgm4D2fgvv3jcTy3DzUHCbu14J6pe7RYqxHA5FGB1",
    "AXjn1qHYrAad5nVznXm7Txyp8WmdzJEEi5NM2axSLtnL",
    "CAiSJtSZfA5usmP9ikPz69nzJLMejbjo7CrGQLwSXR8h",
    "HcEnLVJxG126a68FX6pzw252pL9jUrxkucsbth8hzh9U",
    "3bYr6BzwMqrrH1N3BGjYeEPRrRxde25zj3Garud4qom3",
  ],
  "Serum: BTC / USDC"
);

set(
  [
    "8AcVjMG2LTbpkjNoyq8RwysokqZunkjy3d5JDzxC6BJa",
    "DoYVFapBvZksM6DqqsWgLrqYQL5JknhnjFcEPb2jCr6D",
    "6iveBLh1CWrE1T4p7AE1tX3DPSpnbPLxCPZ2RxTfeTvT",
    "5N2uRKMNDv22rgV8Cm8VtyTByvKGfdFBTtUToqga3bxR",
    "7g8U7LqXbMcRyLnU3KhDNJKrq4VTDbvcVTL3Qt1onKJD",
    "5piQPUuxpGR1HVDA7vMJneH3bcogmXJo7crgNmwaP49D",
    "5Gb6ngdANPa3upkaGbcR9ib55PcsktDhiQcsVr3XCZhj",
    "4rJea7ape61LD6CredX9Xd5mWpKQiJrcWwencsUe1s5r",
  ],
  "Serum: BTC / USDT"
);

set(
  [
    "ASKiV944nKg1W9vsf7hf3fTsjawK6DwLwrnB2LH9n61c",
    "BWqhtYi9sQgN8wfsJNBkKA5fMmbmctFwjUL1xZSHEEdm",
    "PzjG3J5Lzm2P9BaeHHogxacp1BDMtawvrz88fWuadT4",
    "9HqtE85iR2DFMDduCAZe9DG9ygckE7WgmVmbdvKVj3x1",
    "tSZM7972KpDL1WV2r9fTwGhaoPWhJ8nMnEzuvhW9b6Y",
    "HSzt9nNG6MuD1iojc76Ke3eeZ9tN4nnd2NduR3ZccE48",
    "Hmxx3y5EiaaYrC65uoxmsP97TTDs7ffHAi44oiKxTypU",
    "4PfXQMYgQPZe7Z6SuAFKmRiM2jNMi9XbN2vbMS2BhwSm",
  ],
  "Serum: ETH / USDC"
);

set(
  [
    "HfCZdJ1wfsWKfYP2qyWdXTT5PWAGWFctzFjLH48U1Hsd",
    "78dUr2kcuvDnsxTb8caFaGeD8GBmreReSLevDEJjVJmf",
    "2GQd5e5z5BvsPyqSVVUs5Qgk8uSPf43MWnKAMfEQRPH7",
    "888UP9HWwY1vaCMpNpP4nzQXgn7hpqomaFwQcV95FkYB",
    "RCDCybQ7ivCzZPLWQWXNmQo1b6kyAbrkYZStnpJC3Dd",
    "E67Asz53dn73aw7Eju1UdL72qVGTK4Svv3Bk7BYdmwTa",
    "J9EEc8dgJyygNWwN8xzsMh342hS58JRyuX4mBqKNUyQn",
    "9UY946SraowE1R97at4qc8NXfJdr75mBLTbjesN4JKb8",
  ],
  "Serum: ETH / USDT"
);

set(
  [
    "68J6nkWToik6oM9rTatKSR5ibVSykAtzftBUEAvpRsys",
    "GPwEA9RMXAk2ovJZke6xczWt9HM9NzAAC5GD8JeTNRfL",
    "2N1a9yuTRePYzjszUHr2qyjhrKdH3UqUosEbW9sRm5Sq",
    "GKiV6ogZunNyk5nhcRZLTmDbXztDynyB674apQgGo5ve",
    "CASsL5nemAKjD6qCPKXWmfWK1WHQXARVoBUynmTnfgS2",
    "7pzWoBvBdQkF8SLA2PjYNX2aPddPCSnqQ8LTefvQAaq2",
    "Gge5vGgh15dWeJPJSQyErqiTVffUNkctteAoT4tHjRFB",
    "AZVmspyVcUv78HX9PtMFr5guSswotsV6Afsr7Vaifz2P",
  ],
  "Serum: SRM / USDC"
);

set(
  [
    "HARFLhSq8nECZk4DVFKvzqXMNMA9a3hjvridGMFizeLa",
    "3p86S1RzNo7sCVBD3awQ9M88C9bHdgCeDirU2bDv7P3n",
    "yaJV7SqjmDyQXkpEtyxF4R1k3UdVJtdKse7RHB3hKLE",
    "4KoZ6w5hbUGUgBJyAvUZAtHx2MCDyoGrFXZyAXrpu3ep",
    "CS3k6C8gWUupTBeEeh8AyK8fHN16Yj9zXaqLPh3Q4Ti8",
    "8u5NUxd5ShNe5LNv42MXqJ7uuWgAigGe9b5DueLttTh7",
    "FvTgrXUUD2KiLBybdPa3zG5tJJuCD3qyrJsennNRc58W",
    "7SjKY8GoSVTNZa26gNyMtgdUMEKYcxNPtbGVFTmiFbhS",
  ],
  "Serum: SRM / USDT"
);

set(
  [
    "FZqrBXz7ADGsmDf1TM9YgysPUfvtG8rJiNUrqDpHc9Au",
    "2spxPVwPaMWruDHiFfwbchwWG6Lx2soxaCGhjK6DFL1a",
    "9YbacjStsjbAL2P2tjn3TEALBABBh2jsroq911bEbmva",
    "8Vwf6LVGyB5XcxRUPC1S4U2U3GkoEf9x6yTLhwGE611y",
    "GjasUQFVeXcaGcZk5qFRfdWpbbEMcAbv2EVSFxpu6EDR",
    "6HmxKKjmmRZZqUYTH63whUWWF7vdNQfiKx2BtUrtFB3A",
    "2mULsMxTia5uAiFi8huEQ3zeCe8UBXRdnVaQYybnsJ6L",
    "G4FCN83ZSRh4baXDY8eNLhk8reWxKZke3dnDBPmev1Bk",
  ],
  "Serum: FTC / USDC"
);

set(
  [
    "DHDdghmkBhEpReno3tbzBPtsxCt6P3KrMzZvxavTktJt",
    "6cXxUcn7yAkgEWAHqXDav7zeEkbGRAC3cR8VpgzD8kxy",
    "GmE8DSdDkEJJfzABt7DRYB2DUrPKSLgjzBMsgHjpKmZX",
    "DiBDJzU91rSGqrTjnVtQgnsJ3dGYQ2TYwspwxsMLzZV4",
    "B2qVWSfy4HFK9tGATeYWFbMZ6fi54DEiMCh5bDGmy9n4",
    "6xKUQy2Ao4KVvyPns88AwPXchemb9EY4YB3r8gczg6gu",
    "DEixXfs1PJ4q8VcNYJas2JFDdgFnKMS5mEj9WKP9Kn1J",
    "F3Wh4ZAEkR8xrdXwzrADLotvJmYjUiQNwWVvm91mAAa8",
  ],
  "Serum: FTT / USDT"
);

set(
  [
    "FJg9FUtbN3fg3YFbMCFiZKjGh5Bn4gtzxZmtxFzmz9kT",
    "13mTW8D1yoyXMVyCKtmVBtH1sSnHBUWCjfjNJp6CEtDf",
    "EzZkAWQ97k5D1Augf1bopK7zf9CwJZ9V6eCua6yNGnNj",
    "5ULcF7rHaMFqmGWBYxcsFGSpo2SfWYzJ4TcMAYWrMG3S",
    "78XjqnsSY9CWibWCiAumfx8oSL7yCjoWA1DuuvN4TfUr",
    "7j3UcJ77HqA7MTpBNYniBtkDs2F5EgxaLv4oRhK1t63Z",
    "AbMjvkKRRa5SSwR1MHrSL5inxuybDDMfL3F5RJuMyaHL",
    "AnuZK381G6gSbfMsWykkD33WwS4BENfL2rqciQhfbN9L",
  ],
  "Serum: YFI / USDC"
);

set(
  [
    "5zu5bTZZvqESAAgFsr12CUMxdQvMrvU9CgvC1GW8vJdf",
    "HRhgyAnVKX5waJWyC9Yd8jiBnEtwxzjKwk2feSVKrWs3",
    "DZ9rDKDzimKwTCspqePzH5768rqxkjEkDdhHT46ZqSiJ",
    "M6cbtnTtDKeehBpie5edWNLX9C3CNFzZA7HBhjevXiJ",
    "C7XehQ7R7R8CrxJkJFhoRtsV4SJyKhYoawqadxxYYs3F",
    "9gFKq8bvkCYwqoLQj6ZXckggS2BEb7Jj6mnUMPxagDVj",
    "8GF7m51UzcvRWwcRcXRpttetB7PwfHdwn9fqE9PCLk6F",
    "Ci5BrxgxTB2nfmsBz5REpDaChJTZk7FhFTBb8jFbfJp5",
  ],
  "Serum: YFI / USDT"
);

set(
  [
    "7GZ59DMgJ7D6dfoJTpszPayTRyua9jwcaGJXaRMMF1my",
    "8wTo4Tn3QfzYwyjoofutKU4CE8i98Zj8AJrSa2BL4vso",
    "4BTeKadBCoTaCrxwSYj36FNiutZFvRMi8bvuPqZA9pKW",
    "GoaFC6qRq7MNy1suxvPSNYeoioN32kad17qbdqqWEnTe",
    "9cLbP6pEwEXDttbgTrPqYzxyXnHFygj29DT1EdtrQFfS",
    "2vXebfXSBhQ1aEV5Nnv4j3eo1RJVuJP7CoWmHNdvPvhk",
    "BnzYkmNRVZ6Q71mvzLwYqkU5B4S2hUdqm1GyU2DfQJEC",
    "Fpg7XoRAfvrxaVSHmuUS8HRfGzBjRmMoWvRDHoW6G3zV",
  ],
  "Serum: LINK / USDC"
);

set(
  [
    "F5xschQBMpu1gD2q1babYEAVJHR1buj1YazLiXyQNqSW",
    "Ga48n8R13mW4jFnUXmejVa4tPpR8nAo3mYKG95qZEUZV",
    "H7ZHuTaMHswJWbJxsCgtV4ar28Kjty2hB1DbZVT3icjB",
    "HAx1rwxYapD4CPm9G3H2hq1bufykJg6LksZjU39HwHov",
    "7sjAfkzD9xCU68dmvCtt5mEFBaFzqF8GrYjKdUDhLnst",
    "5vNPSbGTMUzKPQtDdDBGUCeCip9uD8igj4Erfdzz7YdU",
    "76jYTFWR1qjaxNeNvLmHuLeitBEdpQ9P7QPBa4pMp5ve",
    "BeZc9vWGyNJ5g6gG8JoRSkvY5oJgyjV7ErLajfVDJ3FA",
  ],
  "Serum: LINK / USDT"
);

set(
  [
    "4eM8iy2k7VXec5VxRk8xFRhHw5Cn67m5FjNzCEegWibm",
    "56RPf4XMR1wLhPKnxgr1cko9gpDWwE7i8w5owWLW4qNT",
    "49oPGPoTexM5CLzrbrMNiNbrrRjhBfLvEPTZRzsPYL3g",
    "66oziZDCXpsCJkYHcxucmEp6bCPaRJUPeeAjSwnudkhb",
    "9cy917jmyRQqQCocgtnPv5gjiUMx4x4npUPF2CP9bHxP",
    "9Sfk6Hk9V8FR3kxYNhNt1STKAtWuXifQGKxUJ9g3Np4b",
    "8gDM85fdTxEWYVbBPDQFh1d5gVRTDQ4vhpCai3UTEdS7",
    "5LaKtnftuks8DB8aJGuHrMnY95KJcUhSpJ77pRi5mvGP",
  ],
  "Serum: XRP / USDC"
);

set(
  [
    "H5BtazuKhHtZCZjFTZSRd4W3QZRaRgq6JcQCBLbjkj1o",
    "6tYvWamhBADnQ3rv7BvAUMyKgcJ2CgtNGSw79REXnsc6",
    "eKr9MEvo1Nv1CMYTjUE3cw1qk3infy5VdtaCZEFkZzn",
    "HrmGTNSsAoe7cUdtZPqzZmnTfnS9WvNwwqiAYXi8ZwAW",
    "BVejg9wNCCai4n2EgTpkiPev3gvS15f1AoKTmK9b2pNs",
    "2SmRPibwnar9Gd9byx5nK6D8ikAcEUQn7ZC6yqjTBkDg",
    "3jjzgJWeFk3J57JvSJ6P22GdyQFkBTbTLsWB6eSNf6WJ",
    "GJKaUk5NCHL4LTpP1Yn57qzTdCtjd3mSCyvXdktF4uPx",
  ],
  "Serum: XRP / USDT"
);

set(
  [
    "BGjv1z7GLAHQH9F9Xd6a3idz5JUo1tka8rsLizPvPq5Z",
    "E6EQQpB7nQwbzggSCUUPhjE8C9TGSw5dguQuQPMgr5Dt",
    "8yG8L7hcUtnboi4RcGQ1SBz9njeXzPvJUrmYrysFr1rT",
    "FjPSrcboahL3jGe4GVZDPURQWE8dL3TWVFHNznK6qaXi",
    "Bh5JCiV2uMsNLDiy5oKJ23YmCPwnmeGQ34kGHUF5dhgu",
    "8q42g61uTZ9bH9RFdWJyceBTcsRJyaeehzUtSyYzMbj2",
    "2Rn3quSFg8cKTcHwadmEXoR2dcjqhsF3s3AJXZUDKMJb",
    "BeiNALSUcT23SqwnGKxJ6XUG1jeFcCozKd3SBKVgDRBN",
  ],
  "Serum: SRM / USDC"
);

set(
  [
    "9kJ8YCHZSiqXgfFyT9LoNVFEv8og3C5oN1pPCwmYRgCz",
    "9DsKwjwP1wkg2UaYXESQtbbTYJ6Q888gfuoa1yAuzMAp",
    "BKhdZHfXaVQ1TjG8mNvifJiMxazjRpWcznjCbtxt3YNv",
    "4iy4REdaS3AEWC4fGZmxP121qKTZ659EeKYyP5uB66qj",
    "2BmuY8oAZVUSn9F2r8JGcLcJN8VHc5QFecub7XBx234W",
    "HZ1aXc9KVfGGS9mD2mzxHh5jeJSvQrx1dz4zGWBaGmkE",
    "FEyMWn3KnWVEMeJr8q7nQxfQu3jq15MchkhLq94kEoVh",
    "9sBxruSjQ97z8k42RXAr2grx9S7Fsk2yisbvZE9cNfjG",
  ],
  "Serum: SRM / USDT"
);

function get(address: string, cluster: Cluster): string | undefined {
  if (cluster === Cluster.MainnetBeta) return MARKET_REGISTRY[address];
}

export const SerumMarketRegistry = {
  get,
};
