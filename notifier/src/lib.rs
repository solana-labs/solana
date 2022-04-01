/// To activate Slack, Discord and/or Telegram notifications, define these environment variables
/// before using the `Notifier`
/// ```bash
/// export SLACK_WEBHOOK=...
/// export DISCORD_WEBHOOK=...
/// ```
///
/// Telegram requires the following two variables:
/// ```bash
/// export TELEGRAM_BOT_TOKEN=...
/// export TELEGRAM_CHAT_ID=...
/// ```
///
/// To receive a Twilio SMS notification on failure, having a Twilio account,
/// and a sending number owned by that account,
/// define environment variable before running `solana-watchtower`:
/// ```bash
/// export TWILIO_CONFIG='ACCOUNT=<account>,TOKEN=<securityToken>,TO=<receivingNumber>,FROM=<sendingNumber>'
/// ```
use log::*;
use {
    reqwest::{blocking::Client, StatusCode},
    serde_json::json,
    std::{env, str::FromStr, thread::sleep, time::Duration},
};

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

enum NotificationType {
    Discord(String),
    Slack(String),
    Telegram(TelegramWebHook),
    Twilio(TwilioWebHook),
    Log(Level),
}

pub struct Notifier {
    client: Client,
    notifiers: Vec<NotificationType>,
}

impl Notifier {
    pub fn default() -> Self {
        Self::new("")
    }

    pub fn new(env_prefix: &str) -> Self {
        info!("Initializing {}Notifier", env_prefix);

        let mut notifiers = vec![];

        if let Ok(webhook) = env::var(format!("{}DISCORD_WEBHOOK", env_prefix)) {
            notifiers.push(NotificationType::Discord(webhook));
        }
        if let Ok(webhook) = env::var(format!("{}SLACK_WEBHOOK", env_prefix)) {
            notifiers.push(NotificationType::Slack(webhook));
        }

        if let (Ok(bot_token), Ok(chat_id)) = (
            env::var(format!("{}TELEGRAM_BOT_TOKEN", env_prefix)),
            env::var(format!("{}TELEGRAM_CHAT_ID", env_prefix)),
        ) {
            notifiers.push(NotificationType::Telegram(TelegramWebHook {
                bot_token,
                chat_id,
            }));
        }

        if let Ok(Some(webhook)) = get_twilio_config() {
            notifiers.push(NotificationType::Twilio(webhook));
        }

        if let Ok(log_level) = env::var(format!("{}LOG_NOTIFIER_LEVEL", env_prefix)) {
            match Level::from_str(&log_level) {
                Ok(level) => notifiers.push(NotificationType::Log(level)),
                Err(e) => warn!(
                    "could not parse specified log notifier level string ({}): {}",
                    log_level, e
                ),
            }
        }

        info!("{} notifiers", notifiers.len());

        Notifier {
            client: Client::new(),
            notifiers,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.notifiers.is_empty()
    }

    pub fn send(&self, msg: &str) {
        for notifier in &self.notifiers {
            match notifier {
                NotificationType::Discord(webhook) => {
                    for line in msg.split('\n') {
                        // Discord rate limiting is aggressive, limit to 1 message a second
                        sleep(Duration::from_millis(1000));

                        info!("Sending {}", line);
                        let data = json!({ "content": line });

                        loop {
                            let response = self.client.post(webhook).json(&data).send();

                            if let Err(err) = response {
                                warn!("Failed to send Discord message: \"{}\": {:?}", line, err);
                                break;
                            } else if let Ok(response) = response {
                                info!("response status: {}", response.status());
                                if response.status() == StatusCode::TOO_MANY_REQUESTS {
                                    warn!("rate limited!...");
                                    warn!("response text: {:?}", response.text());
                                    sleep(Duration::from_secs(2));
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
                NotificationType::Slack(webhook) => {
                    let data = json!({ "text": msg });
                    if let Err(err) = self.client.post(webhook).json(&data).send() {
                        warn!("Failed to send Slack message: {:?}", err);
                    }
                }

                NotificationType::Telegram(TelegramWebHook { chat_id, bot_token }) => {
                    let data = json!({ "chat_id": chat_id, "text": msg });
                    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);

                    if let Err(err) = self.client.post(&url).json(&data).send() {
                        warn!("Failed to send Telegram message: {:?}", err);
                    }
                }

                NotificationType::Twilio(TwilioWebHook {
                    account,
                    token,
                    to,
                    from,
                }) => {
                    let url = format!(
                        "https://{}:{}@api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
                        account, token, account
                    );
                    let params = [("To", to), ("From", from), ("Body", &msg.to_string())];
                    if let Err(err) = self.client.post(&url).form(&params).send() {
                        warn!("Failed to send Twilio message: {:?}", err);
                    }
                }
                NotificationType::Log(level) => {
                    log!(*level, "{}", msg)
                }
            }
        }
    }
}
