use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

const RESULTS: usize = 24;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub channel: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub view_count: Option<u64>,
    pub concurrent_view_count: Option<u64>,
    pub live_status: Option<String>,
    pub thumbnails: Vec<Thumbnail>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Thumbnail {
    pub url: String,
    pub width: Option<u32>,
}

#[derive(Deserialize)]
struct Playlist {
    #[serde(default)]
    entries: Vec<Video>,
}

impl Video {
    pub fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }

    pub fn thumbnail_url(&self) -> String {
        self.thumbnails
            .iter()
            .filter(|t| t.width.unwrap_or(0) <= 720 && !t.url.is_empty())
            .max_by_key(|t| t.width.unwrap_or(0))
            .map(|t| t.url.clone())
            .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", self.id))
    }

    pub fn is_live(&self) -> bool {
        self.live_status.as_deref() == Some("is_live")
    }

    pub fn channel_name(&self) -> &str {
        self.channel.as_deref().or(self.uploader.as_deref()).unwrap_or("")
    }

    pub fn meta_line(&self) -> String {
        let views = if self.is_live() {
            self.concurrent_view_count.map(|n| format!("{} watching", compact(n)))
        } else {
            self.view_count.map(|n| format!("{} views", compact(n)))
        };
        match (self.channel_name(), views) {
            ("", Some(v)) => v,
            (c, Some(v)) => format!("{c}  ·  {v}"),
            (c, None) => c.to_owned(),
        }
    }

    pub fn duration_label(&self) -> Option<String> {
        if self.is_live() {
            return Some("LIVE".into());
        }
        let secs = self.duration? as u64;
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        Some(if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") })
    }
}

fn compact(n: u64) -> String {
    let f = n as f64;
    let (v, suffix) = match n {
        0..=999 => return n.to_string(),
        1_000..=999_999 => (f / 1e3, "K"),
        1_000_000..=999_999_999 => (f / 1e6, "M"),
        _ => (f / 1e9, "B"),
    };
    if v < 10.0 { format!("{:.1}{suffix}", v).replace(".0", "") } else { format!("{:.0}{suffix}", v) }
}

#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub ytdlp: Option<PathBuf>,
    pub mpv: Option<PathBuf>,
}

impl Tools {
    pub fn discover() -> Self {
        Self { ytdlp: find_tool("yt-dlp", &fallback_paths("yt-dlp")), mpv: find_tool("mpv", &fallback_paths("mpv")) }
    }
}

pub const INSTALL_HINT: &str = if cfg!(target_os = "macos") {
    "brew install mpv yt-dlp deno"
} else if cfg!(windows) {
    "winget install shinchiro.mpv yt-dlp.yt-dlp DenoLand.Deno"
} else {
    "install mpv and yt-dlp with your package manager"
};

/// Places package managers put these tools that may be missing from `PATH`,
/// e.g. Homebrew when the app is opened from Finder or Spotlight.
fn fallback_paths(name: &str) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return unix_tool_dirs().into_iter().map(|d| d.join(name)).collect();
    }

    let exe = format!("{name}.exe");
    let local = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program_files = env::var_os("ProgramFiles").map(PathBuf::from);
    let mut paths = Vec::new();
    if let Some(local) = &local {
        let winget = local.join("Microsoft").join("WinGet");
        paths.push(winget.join("Links").join(&exe));
        if let Ok(dir) = std::fs::read_dir(winget.join("Packages")) {
            for entry in dir.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&format!("{name}.")) {
                    paths.push(entry.path().join(&exe));
                }
            }
        }
        paths.push(local.join("Programs").join(name).join(&exe));
    }
    if let Some(pf) = &program_files {
        paths.push(pf.join("MPV Player").join(&exe));
        paths.push(pf.join(name).join(&exe));
    }
    paths
}

fn unix_tool_dirs() -> Vec<PathBuf> {
    let mut dirs =
        vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/local/bin")];
    dirs.extend(env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("bin")));
    dirs
}

/// `PATH` for child processes: the current one plus any of `extra` it lacks, so mpv and yt-dlp
/// can find their own helpers (ffmpeg, deno) when the app was started from Finder or the Dock.
fn child_path(current: Option<std::ffi::OsString>, extra: &[PathBuf]) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = current.iter().flat_map(env::split_paths).collect();
    for dir in extra {
        if !dirs.contains(dir) {
            dirs.push(dir.clone());
        }
    }
    env::join_paths(dirs).unwrap_or_default()
}

fn find_tool(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    env::var_os("PATH")
        .into_iter()
        .flat_map(|p| env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join(&file))
        .chain(extra.iter().cloned())
        .find(|p| p.is_file())
}

const INNERTUBE_SEARCH: &str = "https://www.youtube.com/youtubei/v1/search?prettyPrint=false";
const WEB_CLIENT_VERSION: &str = "2.20260901.00.00";
const VIDEOS_ONLY: &str = "EgIQAQ==";

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";

/// HTTP client that trusts the operating system's certificate store, so it keeps working behind
/// corporate TLS inspection (e.g. Cloudflare Zero Trust) whose root certificate the OS trusts.
static AGENT: LazyLock<ureq::Agent> =
    LazyLock::new(|| agent(ureq::tls::RootCerts::PlatformVerifier, Duration::from_secs(8)));

pub fn agent(roots: ureq::tls::RootCerts, timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(roots).build())
            .build(),
    )
}

pub fn http() -> &'static ureq::Agent {
    &AGENT
}

pub fn warm_up() {
    let _ = AGENT.get("https://www.youtube.com/generate_204").call();
}

pub fn search(tools: &Tools, query: &str) -> Result<Vec<Video>, String> {
    let fast = innertube_search(query);
    if matches!(&fast, Ok(v) if !v.is_empty()) {
        return fast;
    }
    match &tools.ytdlp {
        Some(ytdlp) => ytdlp_search(ytdlp, query),
        None => fast,
    }
}

pub fn innertube_search(query: &str) -> Result<Vec<Video>, String> {
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB", "clientVersion": WEB_CLIENT_VERSION, "hl": "en", "gl": "US" } },
        "query": query,
        "params": VIDEOS_ONLY,
    });
    let text = AGENT
        .post(INNERTUBE_SEARCH)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| format!("Search request failed: {e}"))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("Unexpected search response: {e}"))?;
    Ok(parse_innertube(&json))
}

fn parse_innertube(json: &Value) -> Vec<Video> {
    json.pointer("/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|section| section.pointer("/itemSectionRenderer/contents")?.as_array())
        .flatten()
        .filter_map(|item| item.get("videoRenderer"))
        .filter_map(video_from_renderer)
        .collect()
}

fn video_from_renderer(r: &Value) -> Option<Video> {
    let id = r.get("videoId")?.as_str()?.to_owned();
    let live = r.get("badges").and_then(Value::as_array).into_iter().flatten().any(|b| {
        b.pointer("/metadataBadgeRenderer/style").and_then(Value::as_str) == Some("BADGE_STYLE_TYPE_LIVE_NOW")
    });
    let count = runs_text(r.get("viewCountText")).and_then(|s| leading_number(&s));
    let thumbnails = r
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some(Thumbnail {
                url: t.get("url")?.as_str()?.to_owned(),
                width: t.get("width").and_then(Value::as_u64).map(|w| w as u32),
            })
        })
        .collect();
    Some(Video {
        id,
        title: runs_text(r.get("title")).unwrap_or_default(),
        channel: runs_text(r.get("ownerText")),
        uploader: None,
        duration: runs_text(r.get("lengthText")).and_then(|s| parse_clock(&s)),
        view_count: count.filter(|_| !live),
        concurrent_view_count: count.filter(|_| live),
        live_status: live.then(|| "is_live".to_owned()),
        thumbnails,
    })
}

fn runs_text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return Some(s.to_owned());
    }
    let runs = v.get("runs")?.as_array()?;
    Some(runs.iter().filter_map(|r| r.get("text")?.as_str()).collect())
}

fn parse_clock(s: &str) -> Option<f64> {
    s.trim().split(':').try_fold(0u64, |acc, part| Some(acc * 60 + part.parse::<u64>().ok()?)).map(|secs| secs as f64)
}

fn leading_number(s: &str) -> Option<u64> {
    let digits: String = s.split_whitespace().next()?.chars().filter(|c| *c != ',').collect();
    digits.parse().ok()
}

fn ytdlp_search(ytdlp: &Path, query: &str) -> Result<Vec<Video>, String> {
    let mut cmd = Command::new(ytdlp);
    cmd.arg(format!("ytsearch{RESULTS}:{query}")).args(["--flat-playlist", "-J", "--no-warnings"]);
    prepare_child(&mut cmd);
    let out = cmd.output().map_err(|e| format!("Couldn't run yt-dlp: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        return Err(line.trim().to_owned());
    }
    let playlist: Playlist =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("Unexpected yt-dlp output: {e}"))?;
    Ok(playlist.entries.into_iter().filter(|v| !v.id.is_empty()).collect())
}

impl Tools {
    pub fn ytdl_hook_args(&self) -> Option<String> {
        self.ytdlp.as_ref().map(|y| format!("--script-opts=ytdl_hook-ytdl_path={}", y.display()))
    }
}

pub fn as_direct_link(query: &str) -> Option<&str> {
    let q = query.trim();
    let is_link =
        ["youtube.com/watch", "youtu.be/", "youtube.com/shorts/", "youtube.com/live/"].iter().any(|p| q.contains(p));
    is_link.then_some(q)
}

#[cfg(windows)]
pub fn prepare_child(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn prepare_child(cmd: &mut Command) {
    cmd.env("PATH", child_env_path());
}

/// The `PATH` that mpv and yt-dlp are started with.
pub fn child_env_path() -> std::ffi::OsString {
    if cfg!(windows) {
        env::var_os("PATH").unwrap_or_default()
    } else {
        child_path(env::var_os("PATH"), &unix_tool_dirs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_counts() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_000), "1K");
        assert_eq!(compact(1_250), "1.2K");
        assert_eq!(compact(15_400), "15K");
        assert_eq!(compact(2_000_000), "2M");
        assert_eq!(compact(3_400_000_000), "3.4B");
    }

    #[test]
    fn duration_labels() {
        let v = |d: f64| Video { duration: Some(d), ..Default::default() };
        assert_eq!(v(5.0).duration_label().as_deref(), Some("0:05"));
        assert_eq!(v(754.0).duration_label().as_deref(), Some("12:34"));
        assert_eq!(v(3725.0).duration_label().as_deref(), Some("1:02:05"));
        let live = Video { live_status: Some("is_live".into()), ..Default::default() };
        assert_eq!(live.duration_label().as_deref(), Some("LIVE"));
    }

    #[test]
    fn parses_flat_search_output() {
        let json = br#"{"entries":[
            {"id":"abc","title":"T","channel":"C","duration":61.0,"view_count":1500,
             "thumbnails":[{"url":"u360","width":360},{"url":"u720","width":720},{"url":"u1280","width":1280}]},
            {"id":"","title":"skip me"}
        ]}"#;
        let p: Playlist = serde_json::from_slice(json).unwrap();
        let v: Vec<_> = p.entries.into_iter().filter(|v| !v.id.is_empty()).collect();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].thumbnail_url(), "u720");
        assert_eq!(v[0].meta_line(), "C  ·  1.5K views");
    }

    #[test]
    fn parses_innertube_search() {
        let json: Value = serde_json::from_str(
            r#"{"contents":{"twoColumnSearchResultsRenderer":{"primaryContents":{"sectionListRenderer":{"contents":[
                {"itemSectionRenderer":{"contents":[
                    {"videoRenderer":{"videoId":"v1","title":{"runs":[{"text":"Song "},{"text":"One"}]},
                        "ownerText":{"runs":[{"text":"Artist"}]},"lengthText":{"simpleText":"1:02:05"},
                        "viewCountText":{"simpleText":"2,058,886 views"},
                        "thumbnail":{"thumbnails":[{"url":"t360","width":360},{"url":"t720","width":720}]}}},
                    {"adSlotRenderer":{}},
                    {"videoRenderer":{"videoId":"v2","title":{"runs":[{"text":"Radio"}]},
                        "viewCountText":{"runs":[{"text":"9,247"},{"text":" watching"}]},
                        "badges":[{"metadataBadgeRenderer":{"style":"BADGE_STYLE_TYPE_LIVE_NOW"}}]}}
                ]}},
                {"continuationItemRenderer":{}}
            ]}}}}}"#,
        )
        .unwrap();
        let v = parse_innertube(&json);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "v1");
        assert_eq!(v[0].title, "Song One");
        assert_eq!(v[0].duration, Some(3725.0));
        assert_eq!(v[0].meta_line(), "Artist  ·  2.1M views");
        assert_eq!(v[0].thumbnail_url(), "t720");
        assert!(v[1].is_live());
        assert_eq!(v[1].meta_line(), "9.2K watching");
        assert_eq!(v[1].duration_label().as_deref(), Some("LIVE"));
    }

    #[test]
    fn parses_clock_and_counts() {
        assert_eq!(parse_clock("4:05"), Some(245.0));
        assert_eq!(parse_clock("bad"), None);
        assert_eq!(leading_number("1,234 views"), Some(1234));
        assert_eq!(leading_number("No views"), None);
    }

    #[test]
    fn detects_direct_links() {
        assert_eq!(as_direct_link(" https://youtu.be/xyz "), Some("https://youtu.be/xyz"));
        assert!(as_direct_link("https://www.youtube.com/watch?v=abc").is_some());
        assert!(as_direct_link("lofi beats").is_none());
    }

    #[test]
    fn child_path_appends_missing_tool_dirs_once() {
        let dirs = |names: &[&str]| names.iter().map(PathBuf::from).collect::<Vec<_>>();
        let current = env::join_paths(dirs(&["/usr/bin", "/opt/homebrew/bin"])).unwrap();
        let merged = child_path(Some(current), &dirs(&["/opt/homebrew/bin", "/usr/local/bin"]));
        assert_eq!(
            env::split_paths(&merged).collect::<Vec<_>>(),
            dirs(&["/usr/bin", "/opt/homebrew/bin", "/usr/local/bin"])
        );
        let from_nothing = child_path(None, &dirs(&["/opt/homebrew/bin"]));
        assert_eq!(env::split_paths(&from_nothing).collect::<Vec<_>>(), dirs(&["/opt/homebrew/bin"]));
    }
}
