use crate::yt::{self, Tools};
use eframe::egui;
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POLL_EVERY: Duration = Duration::from_millis(250);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: f64,
}

pub enum Control {
    TogglePause,
    SeekTo(f64),
    SetVolume(f64),
}

pub struct Player {
    child: Option<Child>,
    tx: Option<mpsc::Sender<Control>>,
    status: Arc<Mutex<Status>>,
    volume: f64,
}

impl Player {
    pub fn new() -> Self {
        Self { child: None, tx: None, status: Arc::default(), volume: 80.0 }
    }

    pub fn play_audio(&mut self, tools: &Tools, url: &str, ctx: &egui::Context) -> Result<(), String> {
        self.stop();
        let mpv = tools.mpv.as_ref().ok_or("mpv isn't installed")?;
        let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let pipe = pipe_path(session);

        let mut cmd = Command::new(mpv);
        cmd.args(mpv_args(url, &pipe, self.volume, tools.ytdl_hook_args()));
        yt::hide_console(&mut cmd);
        let child = cmd.spawn().map_err(|e| format!("Couldn't start mpv: {e}"))?;

        let status = Arc::new(Mutex::new(Status { volume: self.volume, ..Default::default() }));
        let (tx, rx) = mpsc::channel();
        let (thread_status, ctx) = (status.clone(), ctx.clone());
        std::thread::spawn(move || run_ipc(&pipe, rx, thread_status, ctx));

        self.child = Some(child);
        self.tx = Some(tx);
        self.status = status;
        Ok(())
    }

    pub fn send(&mut self, control: Control) {
        {
            let mut s = self.status.lock().unwrap();
            match control {
                Control::TogglePause => s.paused = !s.paused,
                Control::SeekTo(t) => s.position = Some(t),
                Control::SetVolume(v) => {
                    s.volume = v;
                    self.volume = v;
                }
            }
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(control);
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// `Some(true)` when the track played to the end, `Some(false)` when mpv failed.
    pub fn poll_finished(&mut self) -> Option<bool> {
        let exit = self.child.as_mut()?.try_wait().ok()??;
        self.child = None;
        self.tx = None;
        Some(exit.success())
    }

    pub fn stop(&mut self) {
        self.tx = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

fn mpv_args(url: &str, pipe: &str, volume: f64, ytdl_hook: Option<String>) -> Vec<String> {
    let mut args: Vec<String> =
        [url, "--no-video", "--force-window=no", "--idle=no", "--no-terminal", "--ytdl-format=bestaudio/best"]
            .map(str::to_owned)
            .into();
    args.push(format!("--input-ipc-server={pipe}"));
    args.push(format!("--volume={volume}"));
    args.extend(ytdl_hook);
    args
}

#[cfg(windows)]
fn pipe_path(session: u32) -> String {
    format!(r"\\.\pipe\fastyt-{}-{session}", std::process::id())
}

#[cfg(windows)]
fn connect(path: &str) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).open(path)
}

#[cfg(unix)]
fn pipe_path(session: u32) -> String {
    std::env::temp_dir().join(format!("fastyt-{}-{session}.sock", std::process::id())).to_string_lossy().into_owned()
}

#[cfg(unix)]
fn connect(path: &str) -> io::Result<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(path)
}

fn run_ipc(path: &str, rx: mpsc::Receiver<Control>, status: Arc<Mutex<Status>>, ctx: egui::Context) {
    let started = Instant::now();
    let stream = loop {
        match connect(path) {
            Ok(s) => break s,
            Err(_) if started.elapsed() < CONNECT_TIMEOUT => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => return,
        }
    };
    let mut conn = Ipc { reader: BufReader::new(stream), next_id: 0 };

    loop {
        loop {
            let command = match rx.try_recv() {
                Ok(Control::TogglePause) => json!(["cycle", "pause"]),
                Ok(Control::SeekTo(t)) => json!(["seek", t, "absolute"]),
                Ok(Control::SetVolume(v)) => json!(["set_property", "volume", v]),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            };
            if conn.request(command).is_err() {
                return;
            }
        }

        let polled = (|| -> io::Result<Status> {
            Ok(Status {
                position: conn.request(json!(["get_property", "time-pos"]))?.as_f64(),
                duration: conn.request(json!(["get_property", "duration"]))?.as_f64(),
                paused: conn.request(json!(["get_property", "pause"]))?.as_bool().unwrap_or(false),
                volume: conn.request(json!(["get_property", "volume"]))?.as_f64().unwrap_or(80.0),
            })
        })();
        match polled {
            Ok(s) => *status.lock().unwrap() = s,
            Err(_) => {
                ctx.request_repaint();
                return;
            }
        }
        ctx.request_repaint();
        std::thread::sleep(POLL_EVERY);
    }
}

struct Ipc<S> {
    reader: BufReader<S>,
    next_id: u64,
}

impl<S: io::Read + io::Write> Ipc<S> {
    fn request(&mut self, command: Value) -> io::Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let stream = self.reader.get_mut();
        writeln!(stream, "{}", json!({ "command": command, "request_id": id }))?;
        stream.flush()?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let Ok(reply) = serde_json::from_str::<Value>(&line) else { continue };
            if reply.get("request_id").and_then(Value::as_u64) == Some(id) {
                return Ok(reply.get("data").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    struct FakeMpv {
        replies: Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl FakeMpv {
        fn replying(lines: &[&str]) -> Self {
            let replies = lines.iter().map(|l| format!("{l}\n")).collect::<String>();
            Self { replies: Cursor::new(replies.into_bytes()), sent: Vec::new() }
        }
    }

    impl Read for FakeMpv {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.replies.read(buf)
        }
    }

    impl Write for FakeMpv {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.sent.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn ipc_request_skips_events_and_other_replies() {
        let fake = FakeMpv::replying(&[
            r#"{"event":"playback-restart"}"#,
            "not json",
            r#"{"data":1.0,"request_id":7,"error":"success"}"#,
            r#"{"data":42.5,"request_id":1,"error":"success"}"#,
        ]);
        let mut ipc = Ipc { reader: BufReader::new(fake), next_id: 0 };

        let data = ipc.request(json!(["get_property", "volume"])).unwrap();

        assert_eq!(data, json!(42.5));
        assert_eq!(
            String::from_utf8(ipc.reader.get_ref().sent.clone()).unwrap(),
            "{\"command\":[\"get_property\",\"volume\"],\"request_id\":1}\n"
        );
    }

    #[test]
    fn ipc_request_treats_missing_data_as_null() {
        let fake = FakeMpv::replying(&[r#"{"request_id":1,"error":"property unavailable"}"#]);
        let mut ipc = Ipc { reader: BufReader::new(fake), next_id: 0 };
        assert_eq!(ipc.request(json!(["get_property", "time-pos"])).unwrap(), Value::Null);
    }

    #[test]
    fn ipc_request_fails_when_mpv_goes_away() {
        let mut ipc = Ipc { reader: BufReader::new(FakeMpv::replying(&[])), next_id: 0 };
        let err = ipc.request(json!(["cycle", "pause"])).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn mpv_is_launched_audio_only() {
        let args = mpv_args("https://youtu.be/x", "PIPE", 80.0, Some("--script-opts=ytdl_hook-ytdl_path=Y".into()));
        assert_eq!(
            args,
            [
                "https://youtu.be/x",
                "--no-video",
                "--force-window=no",
                "--idle=no",
                "--no-terminal",
                "--ytdl-format=bestaudio/best",
                "--input-ipc-server=PIPE",
                "--volume=80",
                "--script-opts=ytdl_hook-ytdl_path=Y",
            ]
        );
        assert_eq!(mpv_args("u", "p", 35.5, None).last().unwrap(), "--volume=35.5");
    }

    #[test]
    fn controls_update_status_immediately_without_mpv() {
        let mut player = Player::new();
        assert_eq!(player.poll_finished(), None);

        player.send(Control::TogglePause);
        player.send(Control::SeekTo(12.0));
        player.send(Control::SetVolume(30.0));

        let s = player.status();
        assert!(s.paused);
        assert_eq!(s.position, Some(12.0));
        assert_eq!(s.volume, 30.0);
        assert_eq!(player.volume(), 30.0);
    }
}
