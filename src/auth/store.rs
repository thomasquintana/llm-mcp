use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Credentials {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub client_id: String,
    pub ext_agent_host_id: String,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub scopes: Vec<String>,
    pub expires_at: u64,
}

impl Credentials {
    pub fn validate(&self) -> Result<()> {
        if self.issuer != "https://auth.openai.com"
            || self.subject.is_empty()
            || !self.client_id.starts_with("oaiapp_")
            || self.access_token.is_empty()
            || self.refresh_token.is_empty()
            || !self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct")
        {
            return Err(Error::new(
                "authentication",
                "invalid OAuth registration or missing ChatGPT plan permission",
            ));
        }
        Ok(())
    }
}

pub(crate) struct Store {
    path: PathBuf,
    pub host_id: String,
    _lease: Arc<File>,
}

fn io_error(_: std::io::Error) -> Error {
    Error::new("credential_store", "cannot access protected OAuth state")
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.as_file().sync_all().map_err(io_error)?;
    file.persist(path)
        .map_err(|_| Error::new("credential_store", "cannot atomically replace OAuth state"))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(io_error)?;
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(io_error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
            }
        }
        let lock_path = path.with_extension("lock");
        let lock = private_options().open(lock_path).map_err(io_error)?;
        lock.try_lock().map_err(|_| {
            Error::new(
                "credential_locked",
                "another process owns this OAuth credential record",
            )
        })?;
        let host_path = path.with_extension("host.json");
        let host_id = if host_path.exists() {
            serde_json::from_slice::<String>(&fs::read(&host_path).map_err(io_error)?)
                .map_err(|_| Error::new("credential_store", "invalid host identity"))?
        } else {
            let id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
            atomic_write(
                &host_path,
                &serde_json::to_vec(&id).expect("string serializes"),
            )?;
            id
        };
        if !host_id.starts_with("urn:uuid:") {
            return Err(Error::new("credential_store", "invalid host identity"));
        }
        Ok(Self {
            path: path.into(),
            host_id,
            _lease: Arc::new(lock),
        })
    }

    pub fn read_path(path: &Path) -> Result<Credentials> {
        let metadata = fs::symlink_metadata(path).map_err(io_error)?;
        if !metadata.is_file() || metadata.len() > 256 * 1024 {
            return Err(Error::new(
                "credential_store",
                "OAuth credential file must be a bounded regular file",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(Error::new(
                    "credential_store",
                    "OAuth credential file must have owner-only permissions (0600)",
                ));
            }
        }
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(io_error)?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        let credentials: Credentials = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new("credential_store", "invalid OAuth credential JSON"))?;
        credentials.validate()?;
        Ok(credentials)
    }

    pub fn load(&self) -> Result<Option<Credentials>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let credentials = Self::read_path(&self.path)?;
        if credentials.ext_agent_host_id != self.host_id {
            return Err(Error::new(
                "credential_store",
                "credential host mismatch; use auth import for transfers",
            ));
        }
        Ok(Some(credentials))
    }

    pub fn save(&self, credentials: &Credentials) -> Result<()> {
        credentials.validate()?;
        atomic_write(
            &self.path,
            &serde_json::to_vec_pretty(credentials)
                .map_err(|_| Error::new("credential_store", "cannot serialize OAuth state"))?,
        )
    }

    pub fn import(&self, source: &Path) -> Result<()> {
        let mut credentials = Self::read_path(source)?;
        credentials.ext_agent_host_id.clone_from(&self.host_id);
        self.save(&credentials)
    }

    pub fn clear(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error(e)),
        }
    }
}
