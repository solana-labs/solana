/// A module for managing a Google API access token
use goauth::{
    auth::{JwtClaims, Token},
    credentials::Credentials,
    get_token,
};
use log::*;
use smpl_jwt::Jwt;
use std::time::Instant;

pub use goauth::scopes::Scope;

fn load_credentials() -> Result<Credentials, String> {
    // Use standard GOOGLE_APPLICATION_CREDENTIALS environment variable
    let credentials_file = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .map_err(|_| "GOOGLE_APPLICATION_CREDENTIALS environment variable not found".to_string())?;

    Credentials::from_file(&credentials_file).map_err(|err| {
        format!(
            "Failed to read GCP credentials from {}: {}",
            credentials_file, err
        )
    })
}

pub struct AccessToken {
    credentials: Credentials,
    jwt: Jwt<JwtClaims>,
    token: Option<(Token, Instant)>,
}

impl AccessToken {
    pub fn new(scope: &Scope) -> Result<Self, String> {
        let credentials = load_credentials()?;

        let claims = JwtClaims::new(
            credentials.iss(),
            &scope,
            credentials.token_uri(),
            None,
            None,
        );
        let jwt = Jwt::new(
            claims,
            credentials
                .rsa_key()
                .map_err(|err| format!("Invalid rsa key: {}", err))?,
            None,
        );

        Ok(Self {
            credentials,
            jwt,
            token: None,
        })
    }

    /// The project that this token grants access to
    pub fn project(&self) -> String {
        self.credentials.project()
    }

    /// Call this function regularly, and before calling `access_token()`
    pub async fn refresh(&mut self) {
        if let Some((token, last_refresh)) = self.token.as_ref() {
            if last_refresh.elapsed().as_secs() < token.expires_in() as u64 / 2 {
                return;
            }
        }

        info!("Refreshing token");
        match get_token(&self.jwt, &self.credentials).await {
            Ok(new_token) => {
                info!("Token expires in {} seconds", new_token.expires_in());
                self.token = Some((new_token, Instant::now()));
            }
            Err(err) => {
                warn!("Failed to get new token: {}", err);
            }
        }
    }

    /// Return an access token suitable for use in an HTTP authorization header
    pub fn get(&self) -> Result<String, String> {
        if let Some((token, _)) = self.token.as_ref() {
            Ok(format!("{} {}", token.token_type(), token.access_token()))
        } else {
            Err("Access token not available".into())
        }
    }
}
