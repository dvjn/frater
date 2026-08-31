use std::{
    env, fs,
    io::{ErrorKind, Read, Write},
    net::SocketAddr,
    path::Path,
    time::Duration,
};

use crate::domain::{AuthConfig, OAuthConfig, SmtpSettings};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use rand::Rng;
use sha2::Sha256;
use url::Url;
use zeroize::Zeroizing;

const KEY_ID: &str = "v1";
const PASSWORD_CONCURRENCY: usize = 2;
const DEFAULT_SECRET_KEY: &str = "@data/root.key";
const ROOT_KEY_LENGTH: usize = 32;

/// HKDF info strings. Each derived key must stay independent of the others, so
/// every purpose gets its own label. Changing a label invalidates the key that
/// it derives.
const SESSION_HMAC_INFO: &[u8] = b"frater/v1/session-hmac";
const OAUTH_HMAC_INFO: &[u8] = b"frater/v1/oauth-hmac";
const PASSWORD_PEPPER_INFO: &[u8] = b"frater/v1/password-pepper";

pub struct Config {
    pub http_addr: SocketAddr,
    pub database_url: String,
    pub public_url: Option<String>,
    pub auth: AuthConfig,
    pub oauth: OAuthConfig,
    pub registration_enabled: bool,
    pub smtp: Option<SmtpSettings>,
}

// The bootstrap command only needs the database and the password pepper.
// It must not receive session or token keys.
pub struct BootstrapConfig {
    pub database_url: String,
    pub password_pepper: Zeroizing<Vec<u8>>,
    pub pepper_key_id: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let mut problems = Problems::default();

        let http_addr = problems.check(http_addr());
        let database_url = problems.check(database_url());
        let public_url = problems.check(optional_public_url());
        let root_key = problems.check(root_key());
        let registration_enabled = problems.check(optional_bool("REGISTRATION_ENABLED", false));
        let smtp = problems.check(smtp_config());

        problems.finish()?;

        let root_key = root_key.expect("validated");
        Ok(Self {
            http_addr: http_addr.expect("validated"),
            database_url: database_url.expect("validated"),
            public_url: public_url.expect("validated"),
            auth: AuthConfig {
                session_hmac_key: *derive_key(&root_key, SESSION_HMAC_INFO),
                session_key_id: KEY_ID.to_owned(),
                password_pepper: derive_key(&root_key, PASSWORD_PEPPER_INFO).to_vec(),
                pepper_key_id: KEY_ID.to_owned(),
                password_concurrency: PASSWORD_CONCURRENCY,
                idle_lifetime: Duration::from_secs(30 * 24 * 60 * 60),
                absolute_lifetime: Duration::from_secs(90 * 24 * 60 * 60),
            },
            oauth: OAuthConfig {
                hmac_key: *derive_key(&root_key, OAUTH_HMAC_INFO),
                key_id: KEY_ID.to_owned(),
            },
            registration_enabled: registration_enabled.expect("validated"),
            smtp: smtp.expect("validated"),
        })
    }
}

#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn check<T>(&mut self, result: Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.0.push(
                    error
                        .chain()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(": "),
                );
                None
            }
        }
    }

    fn finish(self) -> Result<()> {
        if self.0.is_empty() {
            return Ok(());
        }
        let list = self
            .0
            .iter()
            .map(|problem| format!("  - {problem}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!("invalid configuration:\n{list}")
    }
}

fn http_addr() -> Result<SocketAddr> {
    match env::var("HTTP_ADDR") {
        Ok(value) => value
            .parse()
            .context("HTTP_ADDR must be a valid socket address"),
        Err(env::VarError::NotPresent) => Ok(SocketAddr::from(([127, 0, 0, 1], 3000))),
        Err(error) => Err(error).context("HTTP_ADDR must be valid Unicode"),
    }
}

fn smtp_config() -> Result<Option<SmtpSettings>> {
    let host = match env::var("SMTP_HOST") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => return Err(error).context("SMTP_HOST must be valid Unicode"),
    };
    if host.is_empty() {
        bail!("SMTP_HOST must not be empty");
    }
    let port = match env::var("SMTP_PORT") {
        Ok(value) => value.parse::<u16>().context("SMTP_PORT must be a port")?,
        Err(env::VarError::NotPresent) => 587,
        Err(error) => return Err(error).context("SMTP_PORT must be valid Unicode"),
    };
    if port == 0 {
        bail!("SMTP_PORT must be 1..=65535");
    }
    let username = optional_var("SMTP_USERNAME")?;
    let password = optional_var("SMTP_PASSWORD")?;
    if username.is_some() != password.is_some() {
        bail!("SMTP_USERNAME and SMTP_PASSWORD must be set together");
    }
    let from = optional_var("SMTP_FROM")?.context("SMTP_FROM is required when SMTP_HOST is set")?;
    if from.parse::<email_address::EmailAddress>().is_err() {
        bail!("SMTP_FROM must be an email address");
    }
    Ok(Some(SmtpSettings {
        host,
        port,
        username,
        password,
        from,
    }))
}

fn optional_var(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("{name} must be valid Unicode")),
    }
}

impl BootstrapConfig {
    pub fn from_env() -> Result<Self> {
        let database_url = database_url()?;
        let root_key = root_key()?;
        Ok(Self {
            database_url,
            password_pepper: Zeroizing::new(derive_key(&root_key, PASSWORD_PEPPER_INFO).to_vec()),
            pepper_key_id: KEY_ID.to_owned(),
        })
    }
}

fn root_key() -> Result<Zeroizing<[u8; 32]>> {
    let value = match env::var("SECRET_KEY") {
        Ok(value) => Zeroizing::new(value),
        Err(env::VarError::NotPresent) => Zeroizing::new(DEFAULT_SECRET_KEY.to_owned()),
        Err(error) => return Err(error).context("SECRET_KEY must be valid Unicode"),
    };
    root_key_from(&value)
}

fn root_key_from(value: &str) -> Result<Zeroizing<[u8; 32]>> {
    let encoded = match value.strip_prefix('@') {
        Some(path) => {
            if path.is_empty() {
                bail!("SECRET_KEY must name a file after `@`");
            }
            read_or_create_key(Path::new(path), ROOT_KEY_LENGTH)?
        }
        None => Zeroizing::new(value.as_bytes().to_vec()),
    };
    decode_root_key(&encoded)
}

fn decode_root_key(encoded: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let text = std::str::from_utf8(encoded).context("the root key must be base64url text")?;
    let bytes = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(text.trim())
            .context("the root key must be base64url text")?,
    );
    let key: [u8; ROOT_KEY_LENGTH] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("the root key must contain exactly 32 bytes"))?;
    Ok(Zeroizing::new(key))
}

fn derive_key(root_key: &[u8; 32], info: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0_u8; 32]);
    Hkdf::<Sha256>::new(None, root_key)
        .expand(info, key.as_mut_slice())
        .expect("32 bytes is a valid HKDF output length");
    key
}

fn database_url() -> Result<String> {
    match env::var("DATABASE_URL") {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => {
            fs::create_dir_all("data").context("failed to create data directory")?;
            Ok("sqlite://data/frater.db?mode=rwc".to_owned())
        }
        Err(error) => Err(error).context("DATABASE_URL must be valid Unicode"),
    }
}

fn read_or_create_key(path: &Path, length: usize) -> Result<Zeroizing<Vec<u8>>> {
    match read_existing_secret(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut bytes = Zeroizing::new(vec![0_u8; length]);
    rand::rng().fill_bytes(bytes.as_mut_slice());
    let key = Zeroizing::new(URL_SAFE_NO_PAD.encode(&*bytes).into_bytes());

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key)
                .with_context(|| format!("failed to write {}", path.display()))?;
            file.sync_all()
                .with_context(|| format!("failed to sync {}", path.display()))?;
            tracing::info!(path = %path.display(), "generated persistent local secret");
            Ok(key)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => read_existing_secret(path)
            .with_context(|| format!("failed to read concurrently-created {}", path.display())),
        Err(error) => Err(error).with_context(|| format!("failed to create {}", path.display())),
    }
}

fn read_existing_secret(path: &Path) -> std::io::Result<Zeroizing<Vec<u8>>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            ErrorKind::PermissionDenied,
            "secret path must be a regular non-symlink file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "secret file permissions must not grant group or other access",
            ));
        }
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "secret file changed during validation",
            ));
        }
        let mut key = Zeroizing::new(Vec::new());
        file.read_to_end(&mut key)?;
        Ok(key)
    }
    #[cfg(not(unix))]
    {
        let mut file = fs::File::open(path)?;
        let mut key = Zeroizing::new(Vec::new());
        file.read_to_end(&mut key)?;
        Ok(key)
    }
}

fn optional_bool(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => value
            .parse::<bool>()
            .with_context(|| format!("{name} must be true or false")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("{name} must be valid Unicode")),
    }
}

fn optional_public_url() -> Result<Option<String>> {
    let value = match env::var("PUBLIC_URL") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => return Err(error).context("PUBLIC_URL must be valid Unicode"),
    };
    let url = Url::parse(&value).context("PUBLIC_URL must be an absolute URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        bail!("PUBLIC_URL must contain only an http(s) scheme and authority");
    }
    Ok(Some(url.origin().ascii_serialization()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_configuration_problem_is_reported_together() {
        let mut problems = Problems::default();
        assert!(problems.check(http_addr().context("outer")).is_some());
        assert!(
            problems
                .check::<()>(Err(anyhow::anyhow!("second")).context("first"))
                .is_none()
        );
        assert!(
            problems
                .check::<()>(Err(anyhow::anyhow!("other")))
                .is_none()
        );
        let message = problems.finish().unwrap_err().to_string();
        assert!(message.contains("- first: second"));
        assert!(message.contains("- other"));
    }

    #[test]
    fn derived_keys_differ_per_purpose_and_are_stable() {
        let root = [7_u8; 32];
        let session = derive_key(&root, SESSION_HMAC_INFO);
        let oauth = derive_key(&root, OAUTH_HMAC_INFO);
        let pepper = derive_key(&root, PASSWORD_PEPPER_INFO);
        assert_ne!(*session, *oauth);
        assert_ne!(*session, *pepper);
        assert_ne!(*oauth, *pepper);
        assert_eq!(*derive_key(&root, SESSION_HMAC_INFO), *session);
        assert_ne!(*derive_key(&[8_u8; 32], SESSION_HMAC_INFO), *session);
    }

    #[test]
    fn a_direct_value_is_the_key_material() {
        let expected = [3_u8; 32];
        let value = URL_SAFE_NO_PAD.encode(expected);
        assert_eq!(*root_key_from(&value).unwrap(), expected);
        assert_eq!(*root_key_from(&format!("  {value}\n")).unwrap(), expected);
    }

    #[test]
    fn a_path_value_makes_and_reads_the_key_file() {
        let dir = std::env::temp_dir().join(format!("frater-key-{}", std::process::id()));
        let path = dir.join("nested").join("root.key");
        let _ = fs::remove_dir_all(&dir);
        let value = format!("@{}", path.display());

        let created = *root_key_from(&value).unwrap();
        assert_eq!(*root_key_from(&value).unwrap(), created);

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(*root_key_from(text.trim()).unwrap(), created);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_default_value_names_a_file() {
        assert_eq!(DEFAULT_SECRET_KEY, "@data/root.key");
        assert!(DEFAULT_SECRET_KEY.starts_with('@'));
    }

    #[test]
    fn a_bad_value_is_reported() {
        assert!(root_key_from("not base64!").is_err());
        assert!(root_key_from(&URL_SAFE_NO_PAD.encode([1_u8; 16])).is_err());
        assert!(root_key_from("@").is_err());
    }
}
