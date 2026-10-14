// Scratch tool (not part of the app build): put this in a new crate with portable-pty = "0.8" and serde_json = "1".
// ptycap COLS ROWS TOTAL_MS OUT [--send MS:ESCAPED_TEXT]... -- CMD ARGS...
// Runs CMD in a ConPTY (no window), saves the raw output, sends keys at the given times.
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'r' => out.push(b'\r'),
                b'n' => out.push(b'\n'),
                b'e' => out.push(0x1b),
                b't' => out.push(b'\t'),
                b'\\' => out.push(b'\\'),
                b'o' => {
                    // \oNN: a control byte, two octal digits (\o17 = Ctrl+O)
                    let v = u8::from_str_radix(&s[i + 2..i + 4], 8).unwrap();
                    out.push(v);
                    i += 2;
                }
                c => {
                    out.push(b'\\');
                    out.push(c);
                }
            }
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cols: u16 = args[1].parse().unwrap();
    let rows: u16 = args[2].parse().unwrap();
    let total: u64 = args[3].parse().unwrap();
    let out = args[4].clone();
    let mut sends: Vec<(u64, Vec<u8>)> = vec![];
    let mut i = 5;
    while i < args.len() && args[i] != "--" {
        if args[i] == "--send" {
            let (ms, text) = args[i + 1].split_once(':').unwrap();
            sends.push((ms.parse().unwrap(), unescape(text)));
            i += 2;
        } else {
            i += 1;
        }
    }
    let cmd_args = &args[i + 1..];
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).unwrap();
    let mut cmd = CommandBuilder::new(&cmd_args[0]);
    for a in &cmd_args[1..] {
        cmd.arg(a);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(std::env::current_dir().unwrap());
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let events = Arc::new(Mutex::new(Vec::<(f64, char, Vec<u8>)>::new()));
    let t0 = Instant::now();
    let ev2 = events.clone();
    let b2 = buf.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            b2.lock().unwrap().extend_from_slice(&chunk[..n]);
            ev2.lock().unwrap().push((t0.elapsed().as_secs_f64(), 'o', chunk[..n].to_vec()));
        }
    });
    let start = Instant::now();
    sends.sort_by_key(|s| s.0);
    let mut idx = 0;
    while start.elapsed() < Duration::from_millis(total) {
        while idx < sends.len() && start.elapsed() >= Duration::from_millis(sends[idx].0) {
            events.lock().unwrap().push((t0.elapsed().as_secs_f64(), 'i', sends[idx].1.clone()));
            let _ = writer.write_all(&sends[idx].1);
            let _ = writer.flush();
            idx += 1;
        }
        std::thread::sleep(Duration::from_millis(20));
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
    }
    let _ = child.kill();
    std::fs::write(&out, &*buf.lock().unwrap()).unwrap();
    if let Ok(path) = std::env::var("PTYCAP_EVENTS") {
        let lines: Vec<String> = events
            .lock()
            .unwrap()
            .iter()
            .map(|(t, k, b)| serde_json::json!([(t * 1e6).round() / 1e6, k.to_string(), String::from_utf8_lossy(b)]).to_string())
            .collect();
        std::fs::write(path, lines.join("
") + "
").unwrap();
    }
    println!("saved {} bytes", buf.lock().unwrap().len());
}
