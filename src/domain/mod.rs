mod account;
mod auth;
mod bodyweight;
mod catalogue;
mod entity;
mod error;
mod mailer;
mod oauth;
mod secrets;
mod workouts;

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, SqliteTransactionMode,
    TransactionOptions, TransactionTrait,
};
use std::sync::Arc;

pub use account::AccountService;
#[cfg(test)]
pub(crate) use auth::Identity;
pub use auth::{AuthConfig, AuthService, Principal, SessionSummary, bootstrap_superuser};
pub use bodyweight::{BodyweightFilter, LogBodyweight, MAX_BODYWEIGHT_G};
pub use catalogue::{
    ExerciseInput, ExerciseMuscleInput, Lookup, MAX_EXERCISE_ASSOCIATIONS, NamedInput, PageRequest,
};
pub use error::DomainError;
#[cfg(test)]
pub(crate) use mailer::tests::{CapturingMailer, extract_code};
pub use mailer::{LogMailer, Mailer, SmtpMailer, SmtpSettings};
pub use oauth::{
    AuthorizationCodeRedemption, AuthorizationCodeRequest, ClientRegistration, ConnectedClient,
    DEVICE_GRANT_TYPE, DeviceAuthorizationRequest, DevicePollError, DeviceTokenRequest,
    IssuedAccessToken, OAuthConfig, OAuthService, RefreshTokenRequest, SCOPES,
    default_registration_scope, granted_scope, normalize_user_code, resource_scopes, scope_allows,
};
pub use secrets::Password;
pub use workouts::{
    AddExerciseSet, CreateWorkoutSession, LogWorkout, LogWorkoutExercise, MAX_RUN_SPLITS,
    ReplaceRun, RunSplit, SessionFilter, StatsRange, Timestamp,
};

#[cfg(test)]
pub(crate) fn test_oauth_principal(user_id: uuid::Uuid, role: &str, scope: &str) -> Principal {
    Principal {
        identity: auth::Identity {
            user_id,
            role: role.into(),
            auth_version: 0,
        },
        transport: auth::PrincipalTransport::OAuthAccessToken {
            token_id: uuid::Uuid::now_v7(),
            context: auth::OAuthPrincipal {
                client_id: uuid::Uuid::now_v7().to_string(),
                issuer: "https://frater.example".into(),
                resource: "https://frater.example/mcp".into(),
                scope: scope.into(),
            },
        },
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}
struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct DomainOptions {
    pub registration_enabled: bool,
    pub mailer: Arc<dyn Mailer>,
}
impl Default for DomainOptions {
    fn default() -> Self {
        Self {
            registration_enabled: false,
            mailer: Arc::new(LogMailer),
        }
    }
}

pub struct Domain {
    db: DatabaseConnection,
    auth: AuthService,
    oauth: OAuthService,
    account: AccountService,
    #[allow(dead_code)]
    clock: Arc<dyn Clock>,
}
impl Domain {
    #[cfg(test)]
    pub async fn new(
        db: DatabaseConnection,
        auth_config: AuthConfig,
        oauth_config: OAuthConfig,
    ) -> Result<Self, DomainError> {
        Self::with_options(db, auth_config, oauth_config, DomainOptions::default()).await
    }
    pub async fn with_options(
        db: DatabaseConnection,
        auth_config: AuthConfig,
        oauth_config: OAuthConfig,
        options: DomainOptions,
    ) -> Result<Self, DomainError> {
        Self::with_clock(
            db,
            auth_config,
            oauth_config,
            options,
            Arc::new(SystemClock),
        )
        .await
    }
    pub async fn with_clock(
        db: DatabaseConnection,
        auth_config: AuthConfig,
        oauth_config: OAuthConfig,
        options: DomainOptions,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, DomainError> {
        let account_config = account::AccountConfig {
            // One-time codes are opaque credentials of the same class as
            // sessions, so they share the session HMAC key under their own
            // domain separator.
            hmac_key: auth_config.session_hmac_key,
            key_id: auth_config.session_key_id.clone(),
            password_pepper: auth_config.password_pepper.clone(),
            pepper_key_id: auth_config.pepper_key_id.clone(),
            registration_enabled: options.registration_enabled,
        };
        let account =
            AccountService::new(db.clone(), account_config, options.mailer, clock.clone())?;
        let auth = AuthService::new(db.clone(), auth_config, clock.clone()).await?;
        let oauth = OAuthService::new(db.clone(), oauth_config, clock.clone())?;
        Ok(Self {
            db,
            auth,
            oauth,
            account,
            clock,
        })
    }
    pub fn auth(&self) -> &AuthService {
        &self.auth
    }
    pub fn account(&self) -> &AccountService {
        &self.account
    }
    pub fn oauth(&self) -> &OAuthService {
        &self.oauth
    }
    pub async fn change_password(
        &self,
        principal: &Principal,
        current: &Password,
        new: &Password,
    ) -> Result<(), DomainError> {
        let session_id = principal.session_id().ok_or(DomainError::Forbidden)?;
        account::check_password_policy(new)?;
        self.auth
            .verify_user_password(principal.user_id(), current)
            .await?;
        self.account
            .change_password(principal.user_id(), session_id, new)
            .await
    }
    pub async fn health(&self) -> Result<(), DomainError> {
        self.db.execute_unprepared("SELECT 1").await?;
        Ok(())
    }
    async fn begin_immediate(&self) -> Result<DatabaseTransaction, DomainError> {
        // SQLite has a single writer. Reserving it before the first read avoids
        // deferred-transaction upgrade races; busy_timeout provides the bounded wait.
        self.db
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await
            .map_err(|error| {
                tracing::debug!(%error, "could not start immediate transaction");
                DomainError::TemporarilyUnavailable
            })
    }
}
