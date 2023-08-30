---
title: Validator Monitoring Best Practices
sidebar_label: Monitoring
---

It is essential that you have monitoring in place on your validator. In the event that your validator is delinquent (behind the rest of the network) you want to respond immediately to fix the issue. One very useful tool to monitor your validator is [`solana-watchtower`](#solana-watchtower).

## Solana Watchtower

Solana Watchtower is an extremely useful monitoring tool that will regularly monitor the health of your validator. It can monitor your validator for delinquency then notify you on your application of choice: Slack, Discord, Telegram or Twilio. Additionally, `solana-watchtower` has the ability to monitor the health of the entire cluster so that you can be aware of any cluster wide problems.

### Getting Started

To get started with Solana Watchtower, run `solana-watchtower --help`. From the help menu, you can see the optional flags and an explanation of the command.

Here is a sample command that will monitor a validator node with an identity public key of `2uTk98rqqwENevkPH2AHHzGHXgeGc1h6ku8hQUqWeXZp`:

```
solana-watchtower --monitor-active-stake --validator-identity \
  2uTk98rqqwENevkPH2AHHzGHXgeGc1h6ku8hQUqWeXZp
```

The command will monitor your validator, but you will not get notifications unless you added the environment variables mentioned in `solana-watchtower --help`. Since getting each of these services setup for notifications is not straight forward, the next section will walk through [setting up watchtower notifications on Telegram](#setup-telegram-notifications).

### Best Practices

It is a best practice to run the `solana-watchtower` command on a separate server from your validator.

In the case that you run `solana-watchtower` on the same computer as your `solana-validator` process, then during catastrophic events like a power outage, you will not be aware of the issue, because your `solana-watchtower` process will stop at the same time as your `solana-validator` process.

Additionally, while running the `solana-watchtower` process manually with environment variables set in the terminal is a good way to test out the command, it is not operationally sound because the process will not be restarted when the terminal closes or during a system restart.

Instead, you could run your `solana-watchtower` command as a system process similar to `solana-validator`. In the system process file, you can specify the environment variables for your bot.

### Setup Telegram Notifications

To send validator health notifications to your Telegram account, we are going to do a few things:

1. Create a bot to send the messages. The bot will be created using BotFather on Telegram
2. Send a message to the bot
3. Create a Telegram group that will get the watchtower notifications
4. Add the environment variables to your command line environment
5. Restart the `solana-watchtower` command

#### Create a Bot Using BotFather

In Telegram, search for `@BotFather`. Send the following message to _@BotFather_: `/newbot`.

Now you will have to come up with a name for the bot. The only requirement is that it cannot have dashes or spaces, and it **must** end in the word `bot`. Many names have already been taken, so you may have to try a few. Once you find an available name, you will get a response from _@BotFather_ that includes a link to chat with the bot as well as a token for the bot. Take note of the token. You will need it when you setup your environment variables.

#### Send a Message to The Bot

Find the bot in Telegram and send it the following message: `/start`. Messaging the bot will help you later when looking for the bot chatroom id.

#### Create Telegram Group

In Telegram, click on the new message icon and then select new group. Find your newly created bot and add the bot to the group. Next, name the group whatever you'd like.

#### Set Environment Variables For Watchtower

Now that you have a bot setup, you will need to set the environment variables for the bot so that watchtower can send notifications.

First, recall the chat message that you got from _@BotFather_. In the message, there was an HTTP API token for your bot. The token will have this format: `389178471:MMTKMrnZB4ErUzJmuFIXTKE6DupLSgoa7h4o`. You will use that token to set the `TELEGRAM_BOT_TOKEN` environment variable. In the terminal where you plan to run `solana-watchtower`, run the following:

```
export TELEGRAM_BOT_TOKEN=<HTTP API Token>
```

Next, you need the chat id for your group so that `solana-watcher` knows where to send the message. First, send a message to your bot in the chat group that you created. Something like `@newvalidatorbot hello`.

Next, in your browser, go to `https://api.telegram.org/bot<HTTP API Token>/getUpdates`. Make sure to replace `<HTTP API TOKEN>` with your API token that you got in the _@BotFather_ message. Also make sure that you include the word `bot` in the URL before the API token. Make the request in the browser.

The response should be in JSON. Search for the string `"chat":` in the JSON. The `id` value of that chat is your `TELEGRAM_CHAT_ID`. It will be a negative number like: `-781559558`. Remember to include the negative sign! If you cannot find `"chat":` in the JSON, then you may have to remove the bot from your chat group and add it again.

With your Telegram chat id in hand, export the environment variable where you plan to run `solana-watchtower`:

```
export TELEGRAM_CHAT_ID=<negative chat id number>
```

#### Restart solana-watchtower

Once your environment variables are set, restart `solana-watchtower`. You should see output about your validator.

To test that your Telegram configuration is working properly, you could stop your validator briefly until it is labeled as delinquent. Up to a minute after the validator is delinquent, you should receive a message in the Telegram group from your bot. Start the validator again and verify that you get another message in your Telegram group from the bot. The message should say `all clear`.