---
title: RPC Websocket API
displayed_sidebar: apiWebsocketMethodsSidebar
hide_table_of_contents: true
---

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `finalized`.

## RPC PubSub WebSocket Endpoint

**Default port:** 8900 e.g. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## Methods

The following methods are supported in the RPC Websocket API:

import AccountSubscribe from "./websocket/\_accountSubscribe.mdx"

<AccountSubscribe />

import AccountUnsubscribe from "./websocket/\_accountUnsubscribe.mdx"

<AccountUnsubscribe />

import BlockSubscribe from "./websocket/\_blockSubscribe.mdx"

<BlockSubscribe />

import BlockUnsubscribe from "./websocket/\_blockUnsubscribe.mdx"

<BlockUnsubscribe />

import LogsSubscribe from "./websocket/\_logsSubscribe.mdx"

<LogsSubscribe />

import LogsUnsubscribe from "./websocket/\_logsUnsubscribe.mdx"

<LogsUnsubscribe />

import ProgramSubscribe from "./websocket/\_programSubscribe.mdx"

<ProgramSubscribe />

import ProgramUnsubscribe from "./websocket/\_programUnsubscribe.mdx"

<ProgramUnsubscribe />

import SignatureSubscribe from "./websocket/\_signatureSubscribe.mdx"

<SignatureSubscribe />

import SignatureUnsubscribe from "./websocket/\_signatureUnsubscribe.mdx"

<SignatureUnsubscribe />

import SlotSubscribe from "./websocket/\_slotSubscribe.mdx"

<SlotSubscribe />

import SlotUnsubscribe from "./websocket/\_slotUnsubscribe.mdx"

<SlotUnsubscribe />

import SlotsUpdatesSubscribe from "./websocket/\_slotsUpdatesSubscribe.mdx"

<SlotsUpdatesSubscribe />

import SlotsUpdatesUnsubscribe from "./websocket/\_slotsUpdatesUnsubscribe.mdx"

<SlotsUpdatesUnsubscribe />

import RootSubscribe from "./websocket/\_rootSubscribe.mdx"

<RootSubscribe />

import RootUnsubscribe from "./websocket/\_rootUnsubscribe.mdx"

<RootUnsubscribe />

import VoteSubscribe from "./websocket/\_voteSubscribe.mdx"

<VoteSubscribe />

import VoteUnsubscribe from "./websocket/\_voteUnsubscribe.mdx"

<VoteUnsubscribe />
