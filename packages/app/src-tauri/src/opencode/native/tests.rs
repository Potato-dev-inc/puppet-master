use super::*;

/// Manual measurement: `cargo test --lib measure_native_startup -- --ignored --nocapture`.
/// Needs `opencode` installed. Prints per-stage ms (serve spawn -> health -> session
/// -> attach spawn -> first TUI output).
#[test]
#[ignore]
fn measure_native_startup() {
    let pre = Instant::now();
    let _ = crate::opencode::keys::apply_active_auth();
    println!("apply_active_auth={}ms", pre.elapsed().as_millis());
    let pre = Instant::now();
    let exe = crate::shell_env::resolve_opencode_executable().expect("opencode installed");
    println!("resolve_exe={}ms ({exe})", pre.elapsed().as_millis());
    let cwd = std::env::temp_dir().to_string_lossy().to_string();
    for run in 0..3 {
        let t0 = Instant::now();
        let port = pick_free_port(PORT_MIN, PORT_MAX).unwrap();
        let base_url = format!("http://127.0.0.1:{port}");
        let mut serve = StdCommand::new(&exe);
        serve
            .args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
            .current_dir(&cwd)
            .env("PATH", crate::shell_env::path_for_spawn());
        let mut child = serve.spawn().unwrap();
        let spawned = t0.elapsed().as_millis();
        client::wait_for_health(&base_url, Duration::from_secs(45)).unwrap();
        let health = t0.elapsed().as_millis();
        let session = client::create_session(&base_url, "measure").unwrap().id;
        let sess = t0.elapsed().as_millis();
        let pair = native_pty_system()
            .openpty(PtySize { rows: 30, cols: 120, pixel_width: 0, pixel_height: 0 })
            .unwrap();
        let mut attach = pair
            .slave
            .spawn_command(build_attach_command(&exe, &base_url, &session, &cwd))
            .unwrap();
        let attached = t0.elapsed().as_millis();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut parser = vt100::Parser::new(30, 120, 0);
            let mut buf = [0u8; 4096];
            let mut first = None;
            loop {
                let n = reader.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                if first.is_none() {
                    first = Some(());
                    let _ = tx.send((true, false));
                }
                parser.process(&buf[..n]);
                if !parser.screen().contents().trim().is_empty() {
                    let _ = tx.send((false, true));
                    break;
                }
            }
        });
        let mut first = 0;
        let mut text = 0;
        while let Ok((raw, vis)) = rx.recv_timeout(Duration::from_secs(30)) {
            if raw {
                first = t0.elapsed().as_millis();
            }
            if vis {
                text = t0.elapsed().as_millis();
                break;
            }
        }
        println!(
            "run={run} serve_spawned={spawned}ms health={health}ms session={sess}ms attach_spawned={attached}ms first_tui_output={first}ms first_visible_text={text}ms"
        );
        crate::pty::proc_tree::kill_process_tree(attach.process_id().unwrap_or(0));
        let _ = attach.kill();
        kill_serve_child(&mut child);
    }
}
