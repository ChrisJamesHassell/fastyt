//! `fastyt --diagnose [youtube-url]`: measures where time goes on this machine and network
//! (HTTPS, search, stream lookup, download speed, silent playback) and prints a plain-text report.

use crate::player::Player;
use crate::yt::{self, Tools, Video};
use eframe::egui;
use serde_json::Value;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use ureq::tls::RootCerts;

const SEARCH_QUERY: &str = "synthwave mix";
const LONG_VIDEO_SECS: f64 = 20.0 * 60.0;
const SPEED_SAMPLE_BYTES: usize = 8 * 1024 * 1024;
const SPEED_SAMPLE_LIMIT: Duration = Duration::from_secs(15);
const PLAYBACK_START_LIMIT: Duration = Duration::from_secs(30);
const PLAYBACK_SAMPLE: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Report(String);

impl Report {
    fn line(&mut self, text: impl AsRef<str>) {
        self.0.push_str(text.as_ref());
        self.0.push('\n');
    }

    fn section(&mut self, title: &str) {
        self.line(format!("\n== {title}"));
    }
}

pub fn run(target: Option<&str>) -> String {
    let mut r = Report::default();
    r.line(format!(
        "fast.yt {} diagnostics ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let tools = Tools::discover();

    r.section("Tools");
    r.line(tool_line("mpv", tools.mpv.as_deref()));
    r.line(tool_line("yt-dlp", tools.ytdlp.as_deref()));
    let path = yt::child_env_path();
    let runtimes: Vec<String> = ["deno", "node", "bun"]
        .iter()
        .filter_map(|name| find_on_path(name, &path).map(|p| format!("{name} ({})", p.display())))
        .collect();
    r.line(match runtimes.as_slice() {
        [] => "JavaScript runtime for yt-dlp: none found. YouTube may throttle downloads; install deno.".to_owned(),
        found => format!("JavaScript runtime for yt-dlp: {}", found.join(", ")),
    });

    r.section("HTTPS to YouTube");
    let system = probe_https(RootCerts::PlatformVerifier);
    let builtin = probe_https(RootCerts::WebPki);
    r.line(format!("with system certificates (what fast.yt uses): {}", describe(&system)));
    r.line(format!("with a built-in certificate list:              {}", describe(&builtin)));
    if system.is_ok() && builtin.is_err() {
        r.line("A proxy on this network re-signs HTTPS traffic (e.g. Cloudflare Zero Trust); fast.yt handles it.");
    }

    r.section("Search");
    let started = Instant::now();
    let results = yt::innertube_search(SEARCH_QUERY);
    match &results {
        Ok(videos) => {
            r.line(format!("{} results for {SEARCH_QUERY:?} in {} ms", videos.len(), started.elapsed().as_millis()))
        }
        Err(e) => r.line(format!("FAILED after {} ms: {e}", started.elapsed().as_millis())),
    }

    let url = match target {
        Some(url) => url.to_owned(),
        None => match results.ok().as_deref().and_then(pick_long_video) {
            Some(video) => {
                r.line(format!("testing with: {} ({})", video.title, video.duration_label().unwrap_or_default()));
                video.url()
            }
            None => {
                r.line("No video to test with; pass a YouTube link: fastyt --diagnose <url>");
                return r.0;
            }
        },
    };

    r.section("Audio stream lookup (yt-dlp)");
    let stream = match resolve_stream(&tools, &url) {
        Ok((stream, elapsed)) => {
            r.line(format!(
                "found in {} ms: format {} ({}, {}), {}",
                elapsed.as_millis(),
                stream.format_id,
                stream.codec,
                stream.bitrate_kbps.map_or("bitrate unknown".to_owned(), |k| format!("{k:.0} kbps")),
                stream.size.map_or("size unknown".to_owned(), |b| format!("{:.1} MB", b as f64 / 1e6))
            ));
            Some(stream)
        }
        Err(e) => {
            r.line(format!("FAILED: {e}"));
            None
        }
    };

    if let Some(stream) = &stream {
        r.section("Download speed (first 8 MB of the stream)");
        match measure_download(stream) {
            Ok(sample) => {
                let rate = sample.bytes as f64 / sample.elapsed.as_secs_f64();
                r.line(format!(
                    "first byte after {} ms; {:.1} MB in {:.1} s = {:.2} MB/s",
                    sample.first_byte.as_millis(),
                    sample.bytes as f64 / 1e6,
                    sample.elapsed.as_secs_f64(),
                    rate / 1e6
                ));
                if let Some(kbps) = stream.bitrate_kbps {
                    r.line(format!("that is {:.0}x faster than this audio needs", rate / (kbps * 1000.0 / 8.0)));
                }
            }
            Err(e) => r.line(format!("FAILED: {e}")),
        }
    }

    r.section("Playback through mpv (audio output disabled)");
    r.line(measure_playback(&tools, &url));
    r.0
}

fn tool_line(name: &str, path: Option<&Path>) -> String {
    let Some(path) = path else { return format!("{name}: NOT FOUND (install with: {})", yt::INSTALL_HINT) };
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    yt::prepare_child(&mut cmd);
    let version = cmd
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_owned))
        .unwrap_or_else(|| "version unknown".to_owned());
    format!("{name}: {version} ({})", path.display())
}

fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
    std::env::split_paths(path).map(|dir| dir.join(&file)).find(|p| p.is_file())
}

fn probe_https(roots: RootCerts) -> Result<Duration, String> {
    let started = Instant::now();
    yt::agent(roots, Duration::from_secs(10))
        .get("https://www.youtube.com/generate_204")
        .call()
        .map(|_| started.elapsed())
        .map_err(|e| e.to_string())
}

fn describe(result: &Result<Duration, String>) -> String {
    match result {
        Ok(elapsed) => format!("ok ({} ms)", elapsed.as_millis()),
        Err(e) => format!("FAILED ({e})"),
    }
}

fn pick_long_video(videos: &[Video]) -> Option<&Video> {
    videos.iter().find(|v| !v.is_live() && v.duration.is_some_and(|d| d >= LONG_VIDEO_SECS)).or(videos.first())
}

struct Stream {
    url: String,
    headers: Vec<(String, String)>,
    format_id: String,
    codec: String,
    bitrate_kbps: Option<f64>,
    size: Option<u64>,
}

fn resolve_stream(tools: &Tools, url: &str) -> Result<(Stream, Duration), String> {
    let ytdlp = tools.ytdlp.as_ref().ok_or("yt-dlp isn't installed")?;
    let mut cmd = Command::new(ytdlp);
    cmd.args(["-J", "--no-playlist", "--no-warnings", "-f", "bestaudio/best", url]);
    yt::prepare_child(&mut cmd);
    let started = Instant::now();
    let out = cmd.output().map_err(|e| format!("couldn't run yt-dlp: {e}"))?;
    let elapsed = started.elapsed();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim().to_owned());
    }
    let json: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("unexpected yt-dlp output: {e}"))?;
    parse_stream(&json).map(|s| (s, elapsed))
}

fn parse_stream(json: &Value) -> Result<Stream, String> {
    let text = |key: &str| json.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(Stream {
        url: text("url").ok_or("yt-dlp returned no stream URL")?,
        headers: json
            .get("http_headers")
            .and_then(Value::as_object)
            .map(|h| h.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect())
            .unwrap_or_default(),
        format_id: text("format_id").unwrap_or_else(|| "?".to_owned()),
        codec: text("acodec").unwrap_or_else(|| "?".to_owned()),
        bitrate_kbps: json.get("abr").and_then(Value::as_f64),
        size: json.get("filesize").or_else(|| json.get("filesize_approx")).and_then(Value::as_u64),
    })
}

struct Sample {
    first_byte: Duration,
    bytes: usize,
    elapsed: Duration,
}

fn measure_download(stream: &Stream) -> Result<Sample, String> {
    let agent = yt::agent(RootCerts::PlatformVerifier, SPEED_SAMPLE_LIMIT + Duration::from_secs(10));
    let mut request = agent.get(&stream.url).header("Range", format!("bytes=0-{}", SPEED_SAMPLE_BYTES - 1));
    for (key, value) in &stream.headers {
        request = request.header(key, value);
    }
    let started = Instant::now();
    let mut response = request.call().map_err(|e| e.to_string())?;
    let first_byte = started.elapsed();
    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut bytes = 0;
    while bytes < SPEED_SAMPLE_BYTES && started.elapsed() < SPEED_SAMPLE_LIMIT {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => bytes += n,
            Err(e) if bytes == 0 => return Err(e.to_string()),
            Err(_) => break,
        }
    }
    Ok(Sample { first_byte, bytes, elapsed: started.elapsed() })
}

fn measure_playback(tools: &Tools, url: &str) -> String {
    let mut player = Player::new();
    if let Err(e) = player.play_silently(tools, url, &egui::Context::default()) {
        return format!("FAILED to start mpv: {e}");
    }
    let started = Instant::now();
    let mut first_audio = None;
    let (mut stalls, mut stalled_for, mut was_buffering) = (0, Duration::ZERO, false);
    let tick = Duration::from_millis(250);
    loop {
        std::thread::sleep(tick);
        if let Some(ok) = player.poll_finished() {
            return format!(
                "mpv exited early ({}) after {:.1} s",
                if ok { "track ended" } else { "error" },
                started.elapsed().as_secs_f64()
            );
        }
        let status = player.status();
        if first_audio.is_none() && status.position.unwrap_or(0.0) > 0.0 {
            first_audio = Some(started.elapsed());
        }
        if status.buffering {
            stalls += usize::from(!was_buffering);
            stalled_for += tick;
        }
        was_buffering = status.buffering;
        match first_audio {
            None if started.elapsed() > PLAYBACK_START_LIMIT => {
                return format!("no audio after {} s", PLAYBACK_START_LIMIT.as_secs());
            }
            Some(at) if started.elapsed() >= at + PLAYBACK_SAMPLE => {
                return format!(
                    "audio started after {:.1} s; then {} stall(s) totalling {:.1} s in {} s of playback",
                    at.as_secs_f64(),
                    stalls,
                    stalled_for.as_secs_f64(),
                    PLAYBACK_SAMPLE.as_secs()
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn video(id: &str, duration: Option<f64>, live: bool) -> Video {
        Video { id: id.into(), duration, live_status: live.then(|| "is_live".to_owned()), ..Default::default() }
    }

    #[test]
    fn picks_first_long_non_live_video() {
        let videos =
            [video("short", Some(200.0), false), video("live", None, true), video("long", Some(3600.0), false)];
        assert_eq!(pick_long_video(&videos).map(|v| v.id.as_str()), Some("long"));
        assert_eq!(pick_long_video(&videos[..1]).map(|v| v.id.as_str()), Some("short"));
        assert!(pick_long_video(&[]).is_none());
    }

    #[test]
    fn parses_ytdlp_stream_json() {
        let s = parse_stream(&json!({
            "url": "https://rr1.googlevideo.com/videoplayback?x=1",
            "http_headers": { "User-Agent": "UA", "Accept": "*/*" },
            "format_id": "251", "acodec": "opus", "abr": 132.5, "filesize": 140000000
        }))
        .unwrap();
        assert_eq!(s.url, "https://rr1.googlevideo.com/videoplayback?x=1");
        assert_eq!(s.headers, [("Accept".to_owned(), "*/*".to_owned()), ("User-Agent".to_owned(), "UA".to_owned())]);
        assert_eq!((s.format_id.as_str(), s.codec.as_str()), ("251", "opus"));
        assert_eq!(s.bitrate_kbps, Some(132.5));
        assert_eq!(s.size, Some(140_000_000));
        assert!(parse_stream(&json!({ "format_id": "251" })).is_err());
    }

    #[test]
    fn finds_executables_on_a_path() {
        let dir = std::env::temp_dir().join(format!("fastyt-diag-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = if cfg!(windows) { "deno.exe" } else { "deno" };
        std::fs::write(dir.join(file), b"").unwrap();
        let path = std::env::join_paths([PathBuf::from("/definitely/missing"), dir.clone()]).unwrap();
        assert_eq!(find_on_path("deno", &path), Some(dir.join(file)));
        assert_eq!(find_on_path("bun", &path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
