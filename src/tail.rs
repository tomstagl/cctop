//! Follow an append-only JSONL file and emit parsed [`Line`]s.
//!
//! Existing content is parsed first, then appends are picked up from `notify`
//! events (kqueue/inotify) with a 1 s poll as a fallback for filesystems that
//! do not deliver events. Truncation or rotation restarts from byte 0.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::transcript::Line;

/// Poll interval when no filesystem event arrives.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// A running tailer. Drop it (or call [`Tailer::stop`]) to end the thread.
pub struct Tailer {
    rx: mpsc::UnboundedReceiver<Line>,
    stop: Arc<AtomicBool>,
    path: PathBuf,
}

impl Tailer {
    /// Start tailing `path`. The file need not exist yet; it is picked up
    /// when created. Existing content is delivered first, in order.
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Tailer> {
        let path = path.as_ref().to_path_buf();
        let (tx, rx) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            path: path.clone(),
            tx,
            stop: stop.clone(),
        };
        std::thread::Builder::new()
            .name(format!("tail:{}", path.display()))
            .spawn(move || worker.run())?;
        Ok(Tailer { rx, stop, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Next line, or `None` once the tailer has stopped.
    pub async fn recv(&mut self) -> Option<Line> {
        self.rx.recv().await
    }

    /// Non-blocking variant for render loops.
    pub fn try_recv(&mut self) -> Option<Line> {
        self.rx.try_recv().ok()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Tailer {
    fn drop(&mut self) {
        self.stop();
    }
}

enum ReadStop {
    /// File shrank: offset reset to 0, caller should re-read immediately.
    Rewound,
    ReceiverGone,
}

struct Worker {
    path: PathBuf,
    tx: mpsc::UnboundedSender<Line>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    fn run(self) {
        // Watch the parent so we also see the file being created or replaced.
        let (ev_tx, ev_rx) = std_mpsc::channel::<()>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = ev_tx.send(());
            }
        })
        .ok();
        if let (Some(w), Some(parent)) = (watcher.as_mut(), self.path.parent()) {
            let _ = w.watch(parent, RecursiveMode::NonRecursive);
        }
        // kqueue only reports writes for a watched *file*, so add that watch
        // as soon as the file exists (and again after it is replaced).
        let mut file_watched = false;

        let mut offset: u64 = 0;
        let mut partial = Vec::new();
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            if !file_watched && self.path.exists() {
                if let Some(w) = watcher.as_mut() {
                    file_watched = w.watch(&self.path, RecursiveMode::NonRecursive).is_ok();
                }
            }
            match self.read_new(&mut offset, &mut partial) {
                Ok(()) => {}
                Err(ReadStop::Rewound) => {
                    // The inode may have changed; re-arm the file watch and
                    // read the new content right away.
                    if let Some(w) = watcher.as_mut() {
                        let _ = w.unwatch(&self.path);
                    }
                    file_watched = false;
                    continue;
                }
                Err(ReadStop::ReceiverGone) => return,
            }
            // Block until an event or the poll interval; drain bursts.
            let _ = ev_rx.recv_timeout(POLL_INTERVAL);
            while ev_rx.try_recv().is_ok() {}
        }
    }

    /// Read everything after `offset`, emit complete lines, keep the tail.
    fn read_new(&self, offset: &mut u64, partial: &mut Vec<u8>) -> Result<(), ReadStop> {
        let Ok(mut file) = File::open(&self.path) else {
            return Ok(());
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < *offset {
            // Truncated or rotated: start over.
            *offset = 0;
            partial.clear();
            return Err(ReadStop::Rewound);
        }
        if len == *offset {
            return Ok(());
        }
        if file.seek(SeekFrom::Start(*offset)).is_err() {
            return Ok(());
        }
        let mut buf = Vec::with_capacity((len - *offset) as usize);
        if file.read_to_end(&mut buf).is_err() {
            return Ok(());
        }
        *offset += buf.len() as u64;
        partial.extend_from_slice(&buf);

        let mut start = 0;
        while let Some(nl) = partial[start..].iter().position(|&b| b == b'\n') {
            let end = start + nl;
            let text = String::from_utf8_lossy(&partial[start..end]);
            let text = text.trim();
            if !text.is_empty() {
                if let Ok(line) = Line::parse(text) {
                    if self.tx.send(line).is_err() {
                        return Err(ReadStop::ReceiverGone);
                    }
                }
            }
            start = end + 1;
        }
        partial.drain(..start);
        Ok(())
    }
}

/// Parse a whole file synchronously (used by `query` and benchmarks).
pub fn parse_file(path: impl AsRef<Path>) -> std::io::Result<Vec<Line>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| Line::parse(l).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Instant;
    use tokio::time::timeout;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cctop-tail-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("session.jsonl")
    }

    fn line(n: usize) -> String {
        format!(r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"{n}"}}"#)
    }

    async fn next(t: &mut Tailer) -> Line {
        timeout(Duration::from_millis(500), t.recv())
            .await
            .expect("line within 500 ms")
            .expect("tailer alive")
    }

    #[tokio::test]
    async fn existing_content_then_fifty_appends_in_order() {
        let path = temp("append");
        let mut f = File::create(&path).unwrap();
        for n in 0..3 {
            writeln!(f, "{}", line(n)).unwrap();
        }
        let mut t = Tailer::open(&path).unwrap();
        for n in 0..3 {
            match next(&mut t).await {
                Line::QueueOperation(q) => assert_eq!(q.timestamp.unwrap(), n.to_string()),
                other => panic!("{other:?}"),
            }
        }
        let start = Instant::now();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        for n in 100..150 {
            writeln!(f, "{}", line(n)).unwrap();
        }
        drop(f);
        for n in 100..150 {
            match next(&mut t).await {
                Line::QueueOperation(q) => assert_eq!(q.timestamp.unwrap(), n.to_string()),
                other => panic!("{other:?}"),
            }
        }
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "{:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn partial_line_is_buffered_until_newline() {
        let path = temp("partial");
        std::fs::write(&path, "").unwrap();
        let mut t = Tailer::open(&path).unwrap();
        let full = line(7);
        let (a, b) = full.split_at(20);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(f, "{a}").unwrap();
        f.flush().unwrap();
        assert!(
            timeout(Duration::from_millis(300), t.recv()).await.is_err(),
            "must wait for newline"
        );
        writeln!(f, "{b}").unwrap();
        assert!(matches!(next(&mut t).await, Line::QueueOperation(_)));
    }

    #[tokio::test]
    async fn truncation_restarts_from_zero() {
        let path = temp("truncate");
        std::fs::write(&path, format!("{}\n{}\n", line(1), line(2))).unwrap();
        let mut t = Tailer::open(&path).unwrap();
        next(&mut t).await;
        next(&mut t).await;
        std::fs::write(&path, format!("{}\n", line(9))).unwrap();
        match timeout(Duration::from_millis(1500), t.recv())
            .await
            .unwrap()
            .unwrap()
        {
            Line::QueueOperation(q) => assert_eq!(q.timestamp.unwrap(), "9"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn file_created_after_open_is_picked_up() {
        let path = temp("late");
        let _ = std::fs::remove_file(&path);
        let mut t = Tailer::open(&path).unwrap();
        std::fs::write(&path, format!("{}\n", line(3))).unwrap();
        assert!(timeout(Duration::from_millis(1500), t.recv())
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn unknown_types_pass_through_and_bad_json_is_skipped() {
        let path = temp("unknown");
        std::fs::write(&path, "{\"type\":\"future\"}\n{not json}\n").unwrap();
        let mut t = Tailer::open(&path).unwrap();
        assert!(matches!(next(&mut t).await, Line::Unknown(_)));
        assert!(timeout(Duration::from_millis(200), t.recv()).await.is_err());
    }

    #[test]
    fn fixture_parses_synchronously() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        assert_eq!(parse_file(p).unwrap().len(), 1463);
    }

    /// `cargo test -- --ignored bench_ten_megabytes`
    #[test]
    #[ignore]
    fn bench_ten_megabytes() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let text = std::fs::read_to_string(src).unwrap();
        let path = temp("bench");
        let mut f = File::create(&path).unwrap();
        while f.metadata().unwrap().len() < 10 * 1024 * 1024 {
            f.write_all(text.as_bytes()).unwrap();
        }
        drop(f);
        let start = Instant::now();
        let n = parse_file(&path).unwrap().len();
        let took = start.elapsed();
        assert!(n >= 5 * 1463);
        assert!(took < Duration::from_millis(500), "10 MB took {took:?}");
    }
}
