pub use goauth::scopes::Scope;
/// A module for managing a Google API access token
use {
    crate::CredentialType,
    goauth::{
        auth::{JwtClaims, Token},
        credentials::Credentials,
    },
    log::*,
    smpl_jwt::Jwt,
    std::{
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            {Arc, RwLock},
        },
        time::Instant,
    },
    tokio::time,
};

fn load_credentials(filepath: Option<String>) -> Result<Credentials, String> {
    let path = match filepath {
        Some(f) => f,
        None => std::env::var("GOOGLE_APPLICATION_CREDENTIALS").map_err(|_| {
            "GOOGLE_APPLICATION_CREDENTIALS environment variable not found".to_string()
        })?,
    };
    Credentials::from_file(&path)
        .map_err(|err| format!("Failed to read GCP credentials from {path}: {err}"))
}

fn load_stringified_credentials(credential: String) -> Result<Credentials, String> {
    Credentials::from_str(&credential).map_err(|err| format!("{err}"))
}

#[derive(Clone)]
pub struct AccessToken {
    credentials: Credentials,
    scope: Scope,
    refresh_active: Arc<AtomicBool>,
    token: Arc<RwLock<(Token, Instant)>>,
}

impl AccessToken {
    pub async fn new(scope: Scope, credential_type: CredentialType) -> Result<Self, String> {
        let credentials = match credential_type {
            CredentialType::Filepath(fp) => load_credentials(fp)?,
            CredentialType::Stringified(s) => load_stringified_credentials(s)?,
        };

        if let Err(err) = credentials.rsa_key() {
            Err(format!("Invalid rsa key: {err}"))
        } else {
            let token = Arc::new(RwLock::new(Self::get_token(&credentials, &scope).await?));
            let access_token = Self {
                credentials,
                scope,
                token,
                refresh_active: Arc::new(AtomicBool::new(false)),
            };
            Ok(access_token)
        }
    }

    /// The project that this token grants access to
    pub fn project(&self) -> String {
        self.credentials.project()
    }

    async fn get_token(
        credentials: &Credentials,
        scope: &Scope,
    ) -> Result<(Token, Instant), String> {
        info!("Requesting token for {:?} scope", scope);
        let claims = JwtClaims::new(
            credentials.iss(),
            scope,
            credentials.token_uri(),
            None,
            None,
        );
        let jwt = Jwt::new(claims, credentials.rsa_key().unwrap(), None);

        let token = goauth::get_token(&jwt, credentials)
            .await
            .map_err(|err| format!("Failed to refresh access token: {err}"))?;

        info!("Token expires in {} seconds", token.expires_in());
        Ok((token, Instant::now()))
    }

    /// Call this function regularly to ensure the access token does not expire
    pub fn refresh(&self) {
        // Check if it's time to try a token refresh
        let token_r = self.token.read().unwrap();
        if token_r.1.elapsed().as_secs() < token_r.0.expires_in() as u64 / 2 {
            debug!("Token is not expired yet");
            return;
        }
        drop(token_r);

        // Refresh already is progress
        let refresh_progress =
            self.refresh_active
                .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed);
        if refresh_progress.is_err() {
            debug!("Token update is already in progress");
            return;
        }

        let credentials = self.credentials.clone();
        let scope = self.scope.clone();
        let refresh_active = Arc::clone(&self.refresh_active);
        let token = Arc::clone(&self.token);
        tokio::spawn(async move {
            match time::timeout(
                time::Duration::from_secs(5),
                Self::get_token(&credentials, &scope),
            )
            .await
            {
                Ok(new_token) => match new_token {
                    Ok(new_token) => {
                        let mut token_w = token.write().unwrap();
                        *token_w = new_token;
                    }
                    Err(err) => error!("Failed to fetch new token: {}", err),
                },
                Err(_timeout) => {
                    warn!("Token refresh timeout")
                }
            }
            refresh_active.store(false, Ordering::Relaxed);
            info!("Token refreshed");
        });
    }

    /// Return an access token suitable for use in an HTTP authorization header
    pub fn get(&self) -> String {
        let token_r = self.token.read().unwrap();
        format!("{} {}", token_r.0.token_type(), token_r.0.access_token())
    }
}
