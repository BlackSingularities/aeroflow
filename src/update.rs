//! Aktualizacje: sprawdzanie najnowszego wydania na GitHubie, pobieranie instalatora
//! z weryfikacją SHA-256 i uruchomienie go (Windows) albo otwarcie strony wydania.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Repozytorium z wydaniami (można nadpisać przy kompilacji zmienną `AEROFLOW_UPDATE_REPO`).
pub const REPO: &str = match option_env!("AEROFLOW_UPDATE_REPO") {
    Some(r) => r,
    None => "BlackSingularities/aeroflow",
};

/// Wersja programu (w wydaniach ustawiana przez CI).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn releases_url() -> String {
    format!("https://github.com/{REPO}/releases")
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let core = s.split(['-', '+']).next()?;
        let mut it = core.split('.').map(|p| p.parse::<u64>());
        let a = it.next()?.ok()?;
        let b = it.next().unwrap_or(Ok(0)).ok()?;
        let c = it.next().unwrap_or(Ok(0)).ok()?;
        Some(Self(a, b, c))
    }

    pub fn current() -> Self {
        Self::parse(VERSION).unwrap_or(Version(0, 0, 0))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Deserialize, Debug, Clone)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Deserialize, Debug, Clone)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub title: String,
    pub notes: String,
    pub url: String,
    pub date: String,
    pub installer: Option<Asset>,
    pub checksum: Option<Asset>,
}

#[derive(Clone, Debug)]
pub enum State {
    Idle,
    Checking,
    UpToDate { manual: bool },
    Available(Release),
    Downloading { release: Release, done: u64, total: u64 },
    Ready { release: Release, path: PathBuf },
    Failed { message: String, manual: bool },
}

#[derive(Clone)]
pub struct Updater {
    state: Arc<Mutex<State>>,
}

impl Default for Updater {
    fn default() -> Self {
        Self { state: Arc::new(Mutex::new(State::Idle)) }
    }
}

/// Zaufane certyfikaty: wbudowane (Mozilla) oraz systemowe — te drugie są potrzebne w sieciach
/// firmowych, w których serwer pośredniczący podpisuje połączenia własnym certyfikatem.
fn tls_config() -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Ok(native) = rustls_native_certs::load_native_certs() {
        roots.add_parsable_certificates(native);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map(|b| b.with_root_certificates(roots).with_no_client_auth());
    match cfg {
        Ok(c) => Arc::new(c),
        Err(_) => Arc::new(rustls::ClientConfig::builder().with_root_certificates(rustls::RootCertStore::empty()).with_no_client_auth()),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .tls_config(tls_config())
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .user_agent(&format!("AeroFlow/{VERSION}"))
        .try_proxy_from_env(true)
        .build()
}

impl Updater {
    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or(State::Idle)
    }

    fn set(&self, s: State, ctx: &eframe::egui::Context) {
        if let Ok(mut g) = self.state.lock() {
            *g = s;
        }
        ctx.request_repaint();
    }

    pub fn dismiss(&self) {
        if let Ok(mut g) = self.state.lock() {
            *g = State::Idle;
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.state(), State::Checking | State::Downloading { .. })
    }

    /// Sprawdza najnowsze wydanie w tle.
    pub fn check(&self, manual: bool, ctx: &eframe::egui::Context) {
        if self.busy() {
            return;
        }
        self.set(State::Checking, ctx);
        let me = self.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let s = match fetch_latest() {
                Ok(Some(r)) if r.version > Version::current() => State::Available(r),
                Ok(_) => State::UpToDate { manual },
                Err(e) => State::Failed { message: e, manual },
            };
            me.set(s, &ctx);
        });
    }

    /// Pobiera instalator do katalogu tymczasowego i weryfikuje sumę kontrolną.
    pub fn download(&self, release: Release, ctx: &eframe::egui::Context) {
        let Some(asset) = release.installer.clone() else {
            ctx.open_url(eframe::egui::OpenUrl::new_tab(&release.url));
            return;
        };
        self.set(State::Downloading { release: release.clone(), done: 0, total: asset.size }, ctx);
        let me = self.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = download_file(&asset, &release, |done, total| {
                if let Ok(mut g) = me.state.lock() {
                    *g = State::Downloading { release: release.clone(), done, total };
                }
                ctx.request_repaint();
            });
            let s = match res {
                Ok(path) => State::Ready { release, path },
                Err(e) => State::Failed { message: e, manual: true },
            };
            me.set(s, &ctx);
        });
    }
}

fn fetch_latest() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let resp = agent().get(&url).set("Accept", "application/vnd.github+json").call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "Nie znaleziono wydań (repozytorium prywatne lub brak publikacji).".to_string(),
        ureq::Error::Status(403, _) => "Przekroczono limit zapytań GitHub — spróbuj później.".to_string(),
        ureq::Error::Status(c, _) => format!("Serwer zwrócił kod {c}."),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io => {
                "Brak połączenia z GitHub — sprawdź dostęp do internetu.".to_string()
            }
            _ => format!("Błąd połączenia: {}", t.message().unwrap_or("nieznany")),
        },
    })?;
    let list: Vec<GhRelease> = resp.into_json().map_err(|e| format!("Niepoprawna odpowiedź serwera: {e}"))?;
    let best = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| Version::parse(&r.tag_name).map(|v| (v, r)))
        .max_by(|a, b| a.0.cmp(&b.0));
    Ok(best.map(|(version, r)| {
        let installer = r
            .assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".exe") && a.name.to_lowercase().contains("setup"))
            .map(|a| Asset { name: a.name.clone(), url: a.browser_download_url.clone(), size: a.size });
        let checksum = installer.as_ref().and_then(|i| {
            r.assets.iter().find(|a| a.name == format!("{}.sha256", i.name)).map(|a| Asset {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
                size: a.size,
            })
        });
        Release {
            title: r.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("AeroFlow {version}")),
            version,
            notes: r.body.unwrap_or_default(),
            url: r.html_url,
            date: r.published_at.map(|d| d.chars().take(10).collect()).unwrap_or_default(),
            installer,
            checksum,
        }
    }))
}

fn download_file(asset: &Asset, release: &Release, progress: impl Fn(u64, u64)) -> Result<PathBuf, String> {
    let resp = agent().get(&asset.url).call().map_err(|e| format!("Pobieranie nie powiodło się: {e}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(asset.size);
    let dir = std::env::temp_dir().join("AeroFlow-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&asset.name);
    let mut file = std::fs::File::create(&path).map_err(|e| format!("Nie można zapisać pliku: {e}"))?;
    let mut reader = resp.into_reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Przerwane pobieranie: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && done != total {
        return Err("Pobrany plik jest niekompletny.".into());
    }
    if let Some(sum) = &release.checksum {
        let text = agent()
            .get(&sum.url)
            .call()
            .map_err(|e| format!("Nie można pobrać sumy kontrolnej: {e}"))?
            .into_string()
            .map_err(|e| e.to_string())?;
        let expected = text.split_whitespace().next().unwrap_or("").to_lowercase();
        let got = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
        if expected != got {
            let _ = std::fs::remove_file(&path);
            return Err("Suma kontrolna SHA-256 się nie zgadza — plik odrzucono.".into());
        }
    }
    Ok(path)
}

/// Uruchamia instalator w trybie cichym (z ponownym uruchomieniem programu po instalacji).
pub fn launch_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new(path)
            .args(["/SILENT", "/SP-", "/NOCANCEL", "/SUPPRESSMSGBOXES", "/RELAUNCH=1"])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Nie można uruchomić instalatora: {e}"))
    }
    #[cfg(not(windows))]
    {
        Err(format!("Instalator Windows zapisano w {} — na tym systemie zainstaluj aktualizację ręcznie.", path.display()))
    }
}

/// Sekcja „Co nowego” dla danej wersji z dołączonego CHANGELOG.md.
pub fn changelog_section(version: &str) -> Option<String> {
    let text = include_str!("../CHANGELOG.md");
    let head = format!("## [{version}]");
    let start = text.find(&head)?;
    let rest = &text[start + head.len()..];
    let body_start = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let rest = &rest[body_start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

pub fn changelog() -> &'static str {
    include_str!("../CHANGELOG.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(Version::parse("v1.10.0").unwrap() > Version::parse("1.9.9").unwrap());
        assert_eq!(Version::parse("v2.0"), Some(Version(2, 0, 0)));
        assert_eq!(Version::parse("1.2.3-beta"), Some(Version(1, 2, 3)));
        assert!(Version::parse("abc").is_none());
    }

    #[test]
    fn changelog_has_current_version() {
        assert!(changelog_section(VERSION).is_some(), "CHANGELOG.md nie ma sekcji ## [{VERSION}]");
    }
}
