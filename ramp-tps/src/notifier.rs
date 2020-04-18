use log::*;
use reqwest::{blocking::Client, StatusCode};
use serde_json::json;
use std::{env, thread::sleep, time::Duration};

/// For each notification
///   1) Log an info level message
///   2) Notify Slack channel if Slack is configured
///   3) Notify Discord channel if Discord is configured
pub struct Notifier {
    buffer: Vec<String>,
    client: Client,
    discord_webhook: Option<String>,
    slack_webhook: Option<String>,
}

impl Notifier {
    pub fn new() -> Self {
        let discord_webhook = env::var("DISCORD_WEBHOOK")
            .map_err(|_| {
                warn!("Discord notifications disabled");
            })
            .ok();
        let slack_webhook = env::var("SLACK_WEBHOOK")
            .map_err(|_| {
                warn!("Slack notifications disabled");
            })
            .ok();
        Notifier {
            buffer: Vec::new(),
            client: Client::new(),
            discord_webhook,
            slack_webhook,
        }
    }

    fn send(&self, msg: &str) {
        if let Some(webhook) = &self.discord_webhook {
            for line in msg.split('\n') {
                // Discord rate limiting is aggressive, limit to 1 message a second to keep
                // it from getting mad at us...
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
                            std::thread::sleep(Duration::from_secs(2));
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        if let Some(webhook) = &self.slack_webhook {
            let data = json!({ "text": msg });
            if let Err(err) = self.client.post(webhook).json(&data).send() {
                warn!("Failed to send Slack message: {:?}", err);
            }
        }
    }

    pub fn buffer(&mut self, msg: String) {
        self.buffer.push(msg);
    }

    pub fn buffer_vec(&mut self, mut msgs: Vec<String>) {
        self.buffer.append(&mut msgs);
    }

    pub fn flush(&mut self) {
        self.notify(&self.buffer.join("\n"));
        self.buffer.clear();
    }

    pub fn notify(&self, msg: &str) {
        info!("{}", msg);
        self.send(msg);
    }
}
