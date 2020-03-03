use log::*;
use reqwest::blocking::Client;
use serde_json::json;
use std::env;

struct TelegramWebHook {
    bot_token: String,
    chat_id: String,
}

#[derive(Debug, Default)]
struct TwilioWebHook {
    account: String,
    token: String,
    to: String,
    from: String,
}

impl TwilioWebHook {
    fn complete(&self) -> bool {
        !(self.account.is_empty()
            || self.token.is_empty()
            || self.to.is_empty()
            || self.from.is_empty())
    }
}

fn get_twilio_config() -> Result<Option<TwilioWebHook>, String> {
    let config_var = env::var("TWILIO_CONFIG");

    if config_var.is_err() {
        info!("Twilio notifications disabled");
        return Ok(None);
    }

    let mut config = TwilioWebHook::default();

    for pair in config_var.unwrap().split(',') {
        let nv: Vec<_> = pair.split('=').collect();
        if nv.len() != 2 {
            return Err(format!("TWILIO_CONFIG is invalid: '{}'", pair));
        }
        let v = nv[1].to_string();
        match nv[0] {
            "ACCOUNT" => config.account = v,
            "TOKEN" => config.token = v,
            "TO" => config.to = v,
            "FROM" => config.from = v,
            _ => return Err(format!("TWILIO_CONFIG is invalid: '{}'", pair)),
        }
    }

    if !config.complete() {
        return Err("TWILIO_CONFIG is incomplete".to_string());
    }
    Ok(Some(config))
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
        let twilio_webhook = get_twilio_config()
            .map_err(|err| panic!("Twilio config error: {}", err))
            .unwrap();

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

        if let Some(TwilioWebHook {
            account,
            token,
            to,
            from,
        }) = &self.twilio_webhook
        {
            let url = format!(
                "https://{}:{}@api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
                account, token, account
            );
            let params = [("To", to), ("From", from), ("Body", &msg.to_string())];
            if let Err(err) = self.client.post(&url).form(&params).send() {
                warn!("Failed to send Twilio message: {:?}", err);
            }
        }
    }
}
