//! Durable sign-ins and proxy passwords in the platform credential store.
//!
//! All native calls run on one dedicated thread. A locked store cannot hold
//! the UI, command loop, or runtime shutdown hostage. Generation checks reject
//! work from before sign-out, including a write that returns after sign-out.
//! Non-secret revocation markers prevent restoration after a failed deletion.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths::AppDirs;

const SERVICE: &str = "io.github.nizarcan.Navifast";
const LEGACY_SERVICE: &str = "rocks.fastpotify.Fastpotify";
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Slot {
    Proxy,
    Navidrome,
}

impl Slot {
    pub const ALL: [Self; 2] = [Self::Proxy, Self::Navidrome];
    pub(crate) fn index(self) -> usize {
        self as usize
    }
    fn name(self) -> &'static str {
        match self {
            Self::Proxy => "proxy-password",
            Self::Navidrome => "navidrome",
        }
    }
}

/// Deliberately has no Debug implementation: it contains usable secrets.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Grant {
    Proxy(ProxyPassword),
    Navidrome(NavidromePassword),
}

/// The secret is bound to its server and account, never to editable preferences.
/// Deliberately has no Debug implementation.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct NavidromePassword {
    pub server: String,
    pub username: String,
    pub password: String,
}

/// No Debug implementation: the password is usable, and the username is private.
/// A saved password belongs to one network endpoint and username.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyPassword {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ProxyPassword {
    fn valid(&self) -> bool {
        let settings = crate::settings::Settings {
            proxy_host: self.host.clone(),
            proxy_port: self.port.to_string(),
            proxy_username: self.username.clone(),
            proxy_password: self.password.clone(),
            ..Default::default()
        };
        settings.proxy_password_record().ok().flatten().as_ref() == Some(self)
    }
}

impl Grant {
    fn valid_for(&self, slot: Slot) -> bool {
        match (slot, self) {
            (Slot::Proxy, Self::Proxy(password)) => password.valid(),
            (Slot::Navidrome, Self::Navidrome(login)) => {
                crate::navidrome::api::server_url(&login.server).is_ok()
                    && !login.username.trim().is_empty()
                    && !login.password.is_empty()
            }
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    slot: Slot,
    grant: Grant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "The system credential store is unavailable. Unlock or enable it to remember this sign-in."
    )]
    Unavailable,
    #[error(
        "The system credential store is locked or access was denied. Unlock it to remember this sign-in."
    )]
    Locked,
    #[error("The system credential store did not respond. This sign-in cannot be remembered yet.")]
    Timeout,
    #[error("The stored sign-in is invalid. Sign in again.")]
    Invalid,
    #[error(
        "Unable to update credential-storage state. Check the application state directory's permissions."
    )]
    Filesystem,
    #[error(
        "The system credential store did not retain the grant. This sign-in cannot be remembered."
    )]
    Verification,
    #[error("The sign-in changed while credential storage was in progress.")]
    Stale,
}

fn native_error(error: keyring_core::Error) -> Error {
    // Some provider errors contain the secret or arbitrary platform data.
    // Never propagate their Debug/Display text into application diagnostics.
    match error {
        keyring_core::Error::NoStorageAccess(_) => Error::Locked,
        _ => Error::Unavailable,
    }
}

trait ProtectedStore: Send {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error>;
    fn delete(&mut self, key: &str) -> Result<(), Error>;
}

#[derive(Default)]
struct NativeStore {
    store: Option<Arc<keyring_core::api::CredentialStore>>,
    legacy_profile: bool,
}

impl NativeStore {
    fn entry(&mut self, key: &str) -> Result<keyring_core::Entry, Error> {
        self.entry_in(
            if self.legacy_profile {
                LEGACY_SERVICE
            } else {
                SERVICE
            },
            key,
        )
    }

    fn entry_in(&mut self, service: &str, key: &str) -> Result<keyring_core::Entry, Error> {
        if self.store.is_none() {
            #[cfg(target_os = "linux")]
            let store = zbus_secret_service_keyring_store::Store::new();
            #[cfg(target_os = "macos")]
            let store = apple_native_keyring_store::keychain::Store::new();
            #[cfg(windows)]
            let store = windows_native_keyring_store::Store::new();
            self.store = Some(store.map_err(native_error)?);
        }
        self.store
            .as_ref()
            .ok_or(Error::Unavailable)?
            .build(service, key, None)
            .map_err(native_error)
    }
}

impl ProtectedStore for NativeStore {
    fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match self.entry(key)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring_core::Error::NoEntry) => {
                let legacy = self.entry_in(LEGACY_SERVICE, key)?;
                let secret = match legacy.get_secret() {
                    Ok(secret) => secret,
                    Err(keyring_core::Error::NoEntry) => return Ok(None),
                    Err(error) => return Err(native_error(error)),
                };
                let current = self.entry(key)?;
                current.set_secret(&secret).map_err(native_error)?;
                if current.get_secret().map_err(native_error)? != secret {
                    return Err(Error::Unavailable);
                }
                // Delete only after reading the replacement back successfully.
                legacy.delete_credential().map_err(native_error)?;
                Ok(Some(secret))
            }
            Err(error) => Err(native_error(error)),
        }
    }
    fn write(&mut self, key: &str, secret: &[u8]) -> Result<(), Error> {
        self.entry(key)?.set_secret(secret).map_err(native_error)
    }
    fn delete(&mut self, key: &str) -> Result<(), Error> {
        for service in [SERVICE, LEGACY_SERVICE] {
            match self.entry_in(service, key)?.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => (),
                Err(error) => return Err(native_error(error)),
            }
        }
        Ok(())
    }
}

type Job = Box<dyn FnOnce(&mut dyn ProtectedStore) + Send>;

struct Inner {
    dirs: AppDirs,
    profile: String,
    generations: [AtomicU64; Slot::ALL.len()],
    // Held only around the tiny local marker files, never a native store call.
    markers: Mutex<()>,
    jobs: mpsc::SyncSender<Job>,
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

/// Permission to read/write one grant for one authorization generation.
#[derive(Clone)]
pub struct Lease {
    store: Store,
    slot: Slot,
    generation: u64,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    version: u32,
    revoked: bool,
}

pub struct Loaded {
    pub grant: Option<Grant>,
    pub warning: Option<Error>,
}

impl Store {
    pub fn new(dirs: AppDirs) -> Self {
        let backend = NativeStore {
            legacy_profile: dirs.is_legacy_profile(),
            ..Default::default()
        };
        Self::with_backend(dirs, Box::new(backend))
    }

    #[cfg(test)]
    pub(crate) fn in_memory(dirs: AppDirs) -> Self {
        tests::memory_store(dirs)
    }

    fn with_backend(dirs: AppDirs, mut backend: Box<dyn ProtectedStore>) -> Self {
        let (jobs, receiver) = mpsc::sync_channel::<Job>(16);
        let runtime = tokio::runtime::Handle::try_current().ok();
        let _ = std::thread::Builder::new()
            .name("navifast-credentials".into())
            .spawn(move || {
                let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
                while let Ok(job) = receiver.recv() {
                    job(backend.as_mut());
                }
            });
        let profile = std::fs::read_to_string(dirs.state.join("credential-profile"))
            .ok()
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .unwrap_or_else(|| {
                format!(
                    "{:x}",
                    Sha256::digest(dirs.state.to_string_lossy().as_bytes())
                )
            });
        Self {
            inner: Arc::new(Inner {
                dirs,
                profile,
                generations: std::array::from_fn(|_| AtomicU64::new(0)),
                markers: Mutex::new(()),
                jobs,
            }),
        }
    }

    pub fn lease(&self, slot: Slot) -> Lease {
        Lease {
            store: self.clone(),
            slot,
            generation: self.inner.generations[slot.index()].load(Ordering::SeqCst),
        }
    }

    /// Invalidate in-flight work before canceling workers or deleting grants.
    pub fn invalidate(&self, slot: Slot) {
        self.inner.generations[slot.index()].fetch_add(1, Ordering::SeqCst);
    }

    /// Persist revocation before contacting the native store. Even a locked
    /// keychain or an interrupted deletion must not sign the user back in.
    pub fn revoke(&self, slot: Slot) -> Result<(), Error> {
        self.invalidate(slot);
        let _guard = self.inner.markers.lock().unwrap_or_else(|p| p.into_inner());
        let marker = self.write_marker(slot, true);
        let legacy = self.remove_legacy(slot);
        marker.and(legacy)
    }

    fn marker_path(&self, slot: Slot) -> PathBuf {
        self.inner
            .dirs
            .state
            .join("credential-storage")
            .join(format!("{}.json", slot.name()))
    }

    fn marker(&self, slot: Slot) -> Result<Option<Marker>, Error> {
        match std::fs::read(self.marker_path(slot)) {
            Ok(bytes) => {
                let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
                if marker.version != 1 {
                    return Err(Error::Invalid);
                }
                Ok(Some(marker))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(Error::Filesystem),
        }
    }

    fn write_marker(&self, slot: Slot, revoked: bool) -> Result<(), Error> {
        let path = self.marker_path(slot);
        let parent = path.parent().ok_or(Error::Filesystem)?;
        std::fs::create_dir_all(parent).map_err(|_| Error::Filesystem)?;
        let bytes = serde_json::to_vec(&Marker {
            version: 1,
            revoked,
        })
        .map_err(|_| Error::Invalid)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes).map_err(|_| Error::Filesystem)?;
        crate::util::replace_file(&temporary, &path).map_err(|_| Error::Filesystem)
    }

    fn legacy_paths(&self, slot: Slot) -> Vec<PathBuf> {
        if slot == Slot::Proxy {
            let path = self.inner.dirs.proxy_secret_file();
            return vec![path.clone(), path.with_extension("tmp")];
        }
        Vec::new()
    }

    fn remove_legacy(&self, slot: Slot) -> Result<(), Error> {
        let mut result = Ok(());
        for path in self.legacy_paths(slot) {
            if let Err(error) = remove_file(&path) {
                result = Err(error);
            }
        }
        result
    }

    fn legacy_proxy(&self) -> Result<Option<Grant>, Error> {
        let text = match std::fs::read_to_string(self.inner.dirs.settings_file()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return match self.inner.dirs.proxy_secret_file().try_exists() {
                    Ok(false) => Ok(None),
                    Ok(true) => Err(Error::Invalid),
                    Err(_) => Err(Error::Filesystem),
                };
            }
            Err(_) => return Err(Error::Filesystem),
        };
        let mut settings: crate::settings::Settings =
            serde_json::from_str(&text).map_err(|_| Error::Invalid)?;
        settings.migrate_proxy(&text);
        match std::fs::read_to_string(self.inner.dirs.proxy_secret_file()) {
            Ok(password) => settings.proxy_password = password,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Filesystem),
        }
        settings
            .proxy_password_record()
            .map(|password| password.map(Grant::Proxy))
            .map_err(|_| Error::Invalid)
    }

    fn legacy(&self, slot: Slot) -> Result<Option<Grant>, Error> {
        if slot == Slot::Proxy {
            return self.legacy_proxy();
        }
        Ok(None)
    }
}

fn remove_file(path: &Path) -> Result<(), Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::Filesystem),
    }
}

impl Lease {
    pub fn current(&self) -> bool {
        self.store.inner.generations[self.slot.index()].load(Ordering::SeqCst) == self.generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    fn check(&self) -> Result<(), Error> {
        if self.current() {
            Ok(())
        } else {
            Err(Error::Stale)
        }
    }
    fn key(&self) -> String {
        format!("{}:{}", self.store.inner.profile, self.slot.name())
    }

    fn request<T: Send + 'static>(
        &self,
        guard_generation: bool,
        operation: impl FnOnce(Self, &mut dyn ProtectedStore) -> Result<T, Error> + Send + 'static,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, Error>> + Send>> {
        let pending = (|| {
            if guard_generation {
                self.check()?;
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let lease = self.clone();
            self.store
                .inner
                .jobs
                .try_send(Box::new(move |backend| {
                    let result = (if guard_generation {
                        lease.check()
                    } else {
                        Ok(())
                    })
                    .and_then(|()| operation(lease.clone(), backend));
                    let result = result.and_then(|value| {
                        if guard_generation {
                            lease.check()?;
                        }
                        Ok(value)
                    });
                    let _ = sender.send(result);
                }))
                .map_err(|_| Error::Unavailable)?;
            Ok::<_, Error>(receiver)
        })();
        Box::pin(async move {
            tokio::time::timeout(TIMEOUT, pending?)
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Unavailable)?
        })
    }

    pub async fn load(&self) -> Result<Loaded, Error> {
        self.request(true, |lease, backend| {
            let marker = lease.store.marker(lease.slot)?;
            if marker.as_ref().is_some_and(|marker| marker.revoked) {
                return Ok(Loaded {
                    grant: None,
                    warning: None,
                });
            }
            let existing = backend.read(&lease.key());
            if let Ok(Some(bytes)) = &existing {
                let record: Record = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
                if record.version != 1
                    || record.slot != lease.slot
                    || !record.grant.valid_for(lease.slot)
                {
                    return Err(Error::Invalid);
                }
                let _guard = lease
                    .store
                    .inner
                    .markers
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                lease.check()?;
                lease.store.write_marker(lease.slot, false)?;
                let warning = lease.store.remove_legacy(lease.slot).err();
                return Ok(Loaded {
                    grant: Some(record.grant),
                    warning,
                });
            }
            if marker.is_some() {
                return existing.map(|_| Loaded {
                    grant: None,
                    warning: None,
                });
            }
            let Some(grant) = lease.store.legacy(lease.slot)? else {
                return existing.map(|_| Loaded {
                    grant: None,
                    warning: None,
                });
            };
            // Keep a failed migration recoverable. No new plaintext is written,
            // and the caller must show the warning about persistence.
            let warning = match existing {
                Err(error) => Some(error),
                Ok(_) => lease.save_inner(backend, grant.clone()).err(),
            };
            Ok(Loaded {
                grant: Some(grant),
                warning,
            })
        })
        .await
    }

    fn save_inner(&self, backend: &mut dyn ProtectedStore, grant: Grant) -> Result<(), Error> {
        self.check()?;
        if !grant.valid_for(self.slot) {
            return Err(Error::Invalid);
        }
        let bytes = serde_json::to_vec(&Record {
            version: 1,
            slot: self.slot,
            grant,
        })
        .map_err(|_| Error::Invalid)?;
        backend.write(&self.key(), &bytes)?;
        if !self.current() {
            // Operations on this store are serialized. No newer write can be
            // erased here; it is still queued behind this one.
            let _ = backend.delete(&self.key());
            return Err(Error::Stale);
        }
        if backend.read(&self.key())?.as_deref() != Some(bytes.as_slice()) {
            return Err(Error::Verification);
        }
        let _guard = self
            .store
            .inner
            .markers
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.check()?;
        self.store.write_marker(self.slot, false)?;
        self.store.remove_legacy(self.slot)
    }

    /// Enqueue immediately so refreshes preserve write order even when callers
    /// await completion outside their token mutex.
    pub fn save(
        &self,
        grant: Grant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send>> {
        self.request(true, move |lease, backend| lease.save_inner(backend, grant))
    }

    /// Call `Store::revoke` before this. Deletion failure remains visible while
    /// the local revocation marker prevents restoration on the next launch.
    pub fn delete(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send>> {
        // Deletion is enqueued before any replacement write. Run it even if a
        // subsequent authorization increments the generation while it waits.
        self.request(false, |lease, backend| backend.delete(&lease.key()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn navidrome_record_and_key_names_keep_the_installed_format() {
        let original = r#"{"version":1,"slot":"navidrome","grant":{"Navidrome":{"server":"https://music.example.com/","username":"listener","password":"dummy-password"}}}"#;
        let record: Record = serde_json::from_str(original).unwrap();
        assert_eq!(record.version, 1);
        assert_eq!(record.slot.name(), "navidrome");
        assert!(record.grant.valid_for(Slot::Navidrome));
        assert_eq!(serde_json::to_string(&record).unwrap(), original);
        assert_eq!(Slot::Proxy.name(), "proxy-password");
    }

    #[derive(Default)]
    struct Fake {
        values: HashMap<String, Vec<u8>>,
        read_error: Option<Error>,
        write_error: Option<Error>,
        delete_error: Option<Error>,
        discard_write: bool,
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        release: Option<mpsc::Receiver<()>>,
    }
    pub(super) fn memory_store(dirs: AppDirs) -> Store {
        Store::with_backend(
            dirs,
            Box::new(Backend(Arc::new(Mutex::new(Fake::default())))),
        )
    }
    struct Backend(Arc<Mutex<Fake>>);
    impl ProtectedStore for Backend {
        fn read(&mut self, key: &str) -> Result<Option<Vec<u8>>, Error> {
            let fake = self.0.lock().unwrap();
            if let Some(error) = fake.read_error {
                return Err(error);
            }
            Ok(fake.values.get(key).cloned())
        }
        fn write(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
            let (entered, release) = {
                let mut fake = self.0.lock().unwrap();
                if let Some(error) = fake.write_error {
                    return Err(error);
                }
                (fake.entered.take(), fake.release.take())
            };
            if let Some(entered) = entered {
                let _ = entered.send(());
            }
            if let Some(release) = release {
                release.recv().unwrap();
            }
            let mut fake = self.0.lock().unwrap();
            if !fake.discard_write {
                fake.values.insert(key.to_owned(), value.to_vec());
            }
            Ok(())
        }
        fn delete(&mut self, key: &str) -> Result<(), Error> {
            let mut fake = self.0.lock().unwrap();
            if let Some(error) = fake.delete_error {
                return Err(error);
            }
            fake.values.remove(key);
            Ok(())
        }
    }

    struct Fixture {
        dirs: AppDirs,
        fake: Arc<Mutex<Fake>>,
        store: Store,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "navifast-credential-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let dirs = AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            };
            dirs.ensure().unwrap();
            let fake = Arc::new(Mutex::new(Fake::default()));
            let store = Store::with_backend(dirs.clone(), Box::new(Backend(fake.clone())));
            Self { dirs, fake, store }
        }
        fn restart(&self) -> Store {
            Store::with_backend(self.dirs.clone(), Box::new(Backend(self.fake.clone())))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.dirs.state.parent().unwrap());
        }
    }

    fn login(username: &str) -> Grant {
        Grant::Navidrome(NavidromePassword {
            server: "https://music.example.com/".into(),
            username: username.into(),
            password: "dummy-password".into(),
        })
    }

    #[tokio::test]
    async fn renamed_profile_keeps_grants_and_revocation_markers() {
        let fixture = Fixture::new();
        let grant = login("listener");
        fixture
            .store
            .lease(Slot::Navidrome)
            .save(grant.clone())
            .await
            .unwrap();
        fixture.store.revoke(Slot::Proxy).unwrap();
        let parent = fixture.dirs.state.parent().unwrap();
        let renamed = AppDirs {
            config: parent.join("new-config"),
            state: parent.join("new-state"),
            cache: parent.join("new-cache"),
        };
        renamed.migrate_from(&fixture.dirs).unwrap();
        let store = Store::with_backend(renamed, Box::new(Backend(fixture.fake.clone())));
        assert_eq!(store.inner.profile, fixture.store.inner.profile);
        assert!(store.lease(Slot::Navidrome).load().await.unwrap().grant == Some(grant));
        assert!(
            store
                .lease(Slot::Proxy)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }
    fn proxy_password() -> Grant {
        Grant::Proxy(ProxyPassword {
            host: "127.0.0.1".into(),
            port: 8080,
            username: "dummy-user".into(),
            password: " dummy-private-password\n".into(),
        })
    }

    fn navidrome_password() -> Grant {
        Grant::Navidrome(NavidromePassword {
            server: "https://music.example.com/library/".into(),
            username: "dummy-listener".into(),
            password: " dummy-navidrome-password\n".into(),
        })
    }

    #[tokio::test]
    async fn navidrome_login_survives_restart_and_signout_cannot_restore_it() {
        let f = Fixture::new();
        let grant = navidrome_password();
        f.store
            .lease(Slot::Navidrome)
            .save(grant.clone())
            .await
            .unwrap();
        assert!(
            f.restart()
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                == Some(grant)
        );
        let marker = std::fs::read_to_string(f.store.marker_path(Slot::Navidrome)).unwrap();
        assert!(!marker.contains("dummy"));
        assert!(!marker.contains("music.example.com"));
        assert!(f.store.legacy_paths(Slot::Navidrome).is_empty());

        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke(Slot::Navidrome).unwrap();
        assert_eq!(
            f.store.lease(Slot::Navidrome).delete().await,
            Err(Error::Locked)
        );
        f.fake.lock().unwrap().read_error = Some(Error::Locked);
        assert!(
            f.restart()
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn navidrome_locked_store_has_no_plaintext_fallback() {
        let f = Fixture::new();
        f.fake.lock().unwrap().write_error = Some(Error::Locked);
        assert_eq!(
            f.store
                .lease(Slot::Navidrome)
                .save(navidrome_password())
                .await,
            Err(Error::Locked)
        );
        assert!(f.fake.lock().unwrap().values.is_empty());
        assert_eq!(std::fs::read_dir(&f.dirs.config).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&f.dirs.state).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn navidrome_late_save_cannot_undo_signout() {
        let f = Fixture::new();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        {
            let mut fake = f.fake.lock().unwrap();
            fake.entered = Some(entered_tx);
            fake.release = Some(release_rx);
        }
        let saving = f.store.lease(Slot::Navidrome).save(navidrome_password());
        entered_rx.await.unwrap();
        f.store.revoke(Slot::Navidrome).unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(saving.await, Err(Error::Stale));
        assert!(
            f.restart()
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    fn write_legacy(path: &Path, grant: &Grant) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = match grant {
            Grant::Proxy(password) => password.password.as_bytes().to_vec(),
            Grant::Navidrome(_) => panic!("Navidrome has no plaintext legacy credentials"),
        };
        std::fs::write(path, bytes).unwrap();
    }

    fn write_proxy_legacy(f: &Fixture, combined: bool) -> Grant {
        let Grant::Proxy(password) = proxy_password() else {
            unreachable!()
        };
        let settings = crate::settings::Settings {
            proxy_mode: crate::settings::ProxyMode::Http,
            proxy_host: password.host.clone(),
            proxy_port: password.port.to_string(),
            proxy_username: password.username.clone(),
            ..Default::default()
        };
        let mut json = serde_json::to_value(settings).unwrap();
        if combined {
            json["proxy_password"] = password.password.clone().into();
        }
        std::fs::write(f.dirs.settings_file(), serde_json::to_vec(&json).unwrap()).unwrap();
        if !combined {
            std::fs::write(f.dirs.proxy_secret_file(), password.password.as_bytes()).unwrap();
        }
        Grant::Proxy(password)
    }

    #[tokio::test]
    async fn proxy_password_migration_verifies_before_removing_legacy_data() {
        for combined in [false, true] {
            let f = Fixture::new();
            let expected = write_proxy_legacy(&f, combined);
            let settings_before = std::fs::read(f.dirs.settings_file()).unwrap();
            let loaded = f.store.lease(Slot::Proxy).load().await.unwrap();
            assert!(loaded.grant == Some(expected.clone()));
            assert_eq!(loaded.warning, None);
            assert!(!f.dirs.proxy_secret_file().exists());
            // Settings belong to the UI thread. A verified event permits its
            // next atomic save to remove the legacy JSON password.
            assert_eq!(
                std::fs::read(f.dirs.settings_file()).unwrap(),
                settings_before
            );
            assert!(f.restart().lease(Slot::Proxy).load().await.unwrap().grant == Some(expected));
        }
    }

    #[tokio::test]
    async fn a_failed_proxy_migration_retains_the_exact_original_and_can_retry() {
        for combined in [false, true] {
            let f = Fixture::new();
            let expected = write_proxy_legacy(&f, combined);
            let path = if combined {
                f.dirs.settings_file()
            } else {
                f.dirs.proxy_secret_file()
            };
            let before = std::fs::read(&path).unwrap();
            f.fake.lock().unwrap().write_error = Some(Error::Locked);
            let loaded = f.store.lease(Slot::Proxy).load().await.unwrap();
            assert!(
                loaded.grant == Some(expected.clone()),
                "legacy password still serves this session"
            );
            assert_eq!(loaded.warning, Some(Error::Locked));
            assert_eq!(std::fs::read(&path).unwrap(), before);
            f.fake.lock().unwrap().write_error = None;
            let loaded = f.restart().lease(Slot::Proxy).load().await.unwrap();
            assert!(loaded.grant == Some(expected));
            assert_eq!(loaded.warning, None);
        }
    }

    #[tokio::test]
    async fn navidrome_sign_out_retains_the_network_password() {
        let f = Fixture::new();
        let password = proxy_password();
        f.store
            .lease(Slot::Proxy)
            .save(password.clone())
            .await
            .unwrap();
        f.store
            .lease(Slot::Navidrome)
            .save(login("listener"))
            .await
            .unwrap();
        f.store.revoke(Slot::Navidrome).unwrap();
        let restarted = f.restart();
        assert!(restarted.lease(Slot::Proxy).load().await.unwrap().grant == Some(password));
        assert!(
            restarted
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_failed_proxy_delete_cannot_restore_the_forgotten_password() {
        let f = Fixture::new();
        let password = write_proxy_legacy(&f, true);
        f.store.lease(Slot::Proxy).save(password).await.unwrap();
        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke(Slot::Proxy).unwrap();
        assert_eq!(
            f.store.lease(Slot::Proxy).delete().await,
            Err(Error::Locked)
        );
        assert!(
            f.restart()
                .lease(Slot::Proxy)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "requires an unlocked platform credential store; uses dummy grants only"]
    async fn native_store_round_trip() {
        let f = Fixture::new();
        let key = f.store.lease(Slot::Navidrome).key();
        tokio::task::spawn_blocking(move || {
            let mut native = NativeStore::default();
            let legacy = native.entry_in(LEGACY_SERVICE, &key).unwrap();
            legacy.set_secret(b"dummy-rename-probe").unwrap();
            let mut trial = NativeStore {
                legacy_profile: true,
                ..Default::default()
            };
            assert_eq!(
                trial.read(&key).unwrap().as_deref(),
                Some(b"dummy-rename-probe".as_slice())
            );
            assert_eq!(legacy.get_secret().unwrap(), b"dummy-rename-probe");
            assert!(matches!(
                native.entry(&key).unwrap().get_secret(),
                Err(keyring_core::Error::NoEntry)
            ));
            assert_eq!(
                native.read(&key).unwrap().as_deref(),
                Some(b"dummy-rename-probe".as_slice())
            );
            assert!(matches!(
                legacy.get_secret(),
                Err(keyring_core::Error::NoEntry)
            ));
            assert_eq!(
                native.entry(&key).unwrap().get_secret().unwrap(),
                b"dummy-rename-probe"
            );
            native.delete(&key).unwrap();
        })
        .await
        .unwrap();
        let store = Store::new(f.dirs.clone());
        let grants = [proxy_password(), navidrome_password()];
        for (slot, grant) in Slot::ALL.into_iter().zip(grants.iter()) {
            store.lease(slot).save(grant.clone()).await.unwrap();
        }
        let restarted = Store::new(f.dirs.clone());
        for (slot, grant) in Slot::ALL.into_iter().zip(grants) {
            assert!(restarted.lease(slot).load().await.unwrap().grant == Some(grant));
        }
        store.revoke(Slot::Navidrome).unwrap();
        store.revoke(Slot::Proxy).unwrap();
        store.revoke(Slot::Navidrome).unwrap();
        for slot in Slot::ALL {
            let lease = store.lease(slot);
            lease.delete().await.unwrap();
            assert!(
                lease
                    .request(false, |lease, backend| backend.read(&lease.key()))
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn queued_deletion_cannot_erase_a_new_authorization() {
        let f = Fixture::new();
        let old = login("old-account");
        let new = login("new-account");
        f.store.lease(Slot::Navidrome).save(old).await.unwrap();
        f.store.revoke(Slot::Navidrome).unwrap();
        let deletion = f.store.lease(Slot::Navidrome).delete();
        f.store.invalidate(Slot::Navidrome);
        let saving = f.store.lease(Slot::Navidrome).save(new.clone());
        deletion.await.unwrap();
        saving.await.unwrap();
        assert!(
            f.restart()
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                == Some(new)
        );
    }

    #[test]
    fn a_failed_revocation_marker_does_not_prevent_legacy_cleanup() {
        let f = Fixture::new();
        let path = f.dirs.proxy_secret_file();
        write_legacy(&path, &proxy_password());
        std::fs::write(f.dirs.state.join("credential-storage"), b"not a directory").unwrap();
        assert_eq!(f.store.revoke(Slot::Proxy), Err(Error::Filesystem));
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn late_write_after_signout_cannot_recreate_grant() {
        let f = Fixture::new();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        {
            let mut fake = f.fake.lock().unwrap();
            fake.entered = Some(entered_tx);
            fake.release = Some(release_rx);
        }
        let lease = f.store.lease(Slot::Navidrome);
        let pending_lease = lease.clone();
        let pending = tokio::spawn(async move { pending_lease.save(login("listener")).await });
        entered_rx.await.unwrap();
        f.store.revoke(Slot::Navidrome).unwrap();
        assert!(!lease.current());
        release_tx.send(()).unwrap();
        assert_eq!(pending.await.unwrap(), Err(Error::Stale));
        assert!(f.fake.lock().unwrap().values.is_empty());
        assert!(
            f.restart()
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn failed_native_deletion_remains_revoked_across_restart() {
        let f = Fixture::new();
        f.store
            .lease(Slot::Navidrome)
            .save(login("listener"))
            .await
            .unwrap();
        f.fake.lock().unwrap().delete_error = Some(Error::Locked);
        f.store.revoke(Slot::Navidrome).unwrap();
        assert_eq!(
            f.store.lease(Slot::Navidrome).delete().await,
            Err(Error::Locked)
        );
        assert_eq!(f.fake.lock().unwrap().values.len(), 1);
        // A revoked load must not even require access to the locked service.
        f.fake.lock().unwrap().read_error = Some(Error::Locked);
        assert!(
            f.restart()
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
    }

    #[tokio::test]
    async fn profiles_are_isolated_and_invalid_records_are_rejected() {
        let f = Fixture::new();
        let other = Fixture::new();
        let isolated = Store::with_backend(other.dirs.clone(), Box::new(Backend(f.fake.clone())));
        f.store
            .lease(Slot::Navidrome)
            .save(login("listener"))
            .await
            .unwrap();
        assert!(
            isolated
                .lease(Slot::Navidrome)
                .load()
                .await
                .unwrap()
                .grant
                .is_none()
        );
        for bytes in [
            b"not-json".to_vec(),
            serde_json::to_vec(&Record {
                version: 2,
                slot: Slot::Navidrome,
                grant: login("listener"),
            })
            .unwrap(),
            serde_json::to_vec(&Record {
                version: 1,
                slot: Slot::Proxy,
                grant: proxy_password(),
            })
            .unwrap(),
        ] {
            f.fake
                .lock()
                .unwrap()
                .values
                .insert(f.store.lease(Slot::Navidrome).key(), bytes);
            assert!(matches!(
                f.store.lease(Slot::Navidrome).load().await,
                Err(Error::Invalid)
            ));
        }
    }
}
