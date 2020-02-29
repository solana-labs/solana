use log::*;
use reqwest::blocking::Client;
use serde_json::json;
use std::env;

struct TelegramWebHook {
    bot_token: String,
    chat_id: String,
}

struct TwilioWebHook {
    twilio_account: String,
    twilio_token: String,
    twilio_to: String,
    twilio_from: String,
}

pub struct Notifier {
    client: Client,
    discord_webhook: Option<String>,
    slack_webhook: Option<String>,
    telegram_webhook: Option<TelegramWebHook>,
    twilio_webhook: Option<TwilioWebHook>,
}

impl Notifier {
    pub fn new() -> Self {
        let discord_webhook = env::var("DISCORD_WEBHOOK")
            .map_err(|_| {
                info!("Discord notifications disabled");
            })
            .ok();
        let slack_webhook = env::var("SLACK_WEBHOOK")
            .map_err(|_| {
                info!("Slack notifications disabled");
            })
            .ok();
        let telegram_webhook = if let (Ok(bot_token), Ok(chat_id)) =
            (env::var("TELEGRAM_BOT_TOKEN"), env::var("TELEGRAM_CHAT_ID"))
        {
            Some(TelegramWebHook { bot_token, chat_id })
        } else {
            info!("Telegram notifications disabled");
            None
        };
        let twilio_webhook = if let (Ok(twilio_account), Ok(twilio_token), Ok(twilio_to), Ok(twilio_from)) =
            (env::var("TWILIO_ACCOUNT"), env::var("TWILIO_TOKEN"), env::var("TWILIO_TO"), env::var("TWILIO_FROM"))
        {
            Some(TwilioWebHook { twilio_account, twilio_token, twilio_to, twilio_from })
        } else {
            info!("Twilio notifications disabled");
            None
        };

        Notifier {
            client: Client::new(),
            discord_webhook,
            slack_webhook,
            telegram_webhook,
            twilio_webhook,
        }
    }

    pub fn send(&self, msg: &str) {
        if let Some(webhook) = &self.discord_webhook {
            let data = json!({ "content": msg });
            if let Err(err) = self.client.post(webhook).json(&data).send() {
                warn!("Failed to send Discord message: {:?}", err);
            }
        }

        if let Some(webhook) = &self.slack_webhook {
            let data = json!({ "text": msg });
            if let Err(err) = self.client.post(webhook).json(&data).send() {
                warn!("Failed to send Slack message: {:?}", err);
            }
        }

        if let Some(TelegramWebHook { chat_id, bot_token }) = &self.telegram_webhook {
            let data = json!({ "chat_id": chat_id, "text": msg });
            let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);

            if let Err(err) = self.client.post(&url).json(&data).send() {
                warn!("Failed to send Telegram message: {:?}", err);
            }
        }

        if let Some(TwilioWebHook { twilio_account, twilio_token, twilio_to, twilio_from }) = &self.twilio_webhook {
            let url = format!("https://{}:{}@api.twilio.com/2010-04-01/Accounts/{}/Messages.json", twilio_account, twilio_token, twilio_account);
            let params = [("To",twilio_to), ("From",twilio_from), ("Body",&msg.to_string())];
            if let Err(err) = self.client.post(&url).form(&params).send() {
                warn!("Failed to send Twilio message: {:?}", err);
            }
        }

    }
}
