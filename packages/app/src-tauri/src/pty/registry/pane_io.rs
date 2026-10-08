//! Pane I/O: writing input, reading buffers/snapshots, resizing.

use super::*;

pub fn write_input(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    text: &str,
    append_newline: bool,
    via_opencode_api: bool,
    opencode_model: Option<crate::opencode::OpenCodeModelRef>,
) -> Result<(), String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| unknown_pane_message(pane_id))?;

    let agent = pane.info.agent_type.clone();

    if *pane.exited.lock() {
        return Err(format!(
            "pane {pane_id} has exited — close the worker window and run `npm run worker` again"
        ));
    }

    if via_opencode_api
        && agent == "opencode_native"
        && !text.trim().is_empty()
        && !is_pty_control_input(text)
    {
        drop(reg);
        crate::opencode::write_native_input(registry, app, pane_id, text, opencode_model.as_ref())?;
        crate::event_log::append_system_event(SystemEvent::PaneInputWritten {
            pane_id: PaneId(pane_id.to_string()),
            byte_count: text.len(),
            append_newline,
        });
        return Ok(());
    }

    if via_opencode_api && agent == "opencode_native" && text.is_empty() && append_newline {
        // API prompts don't need a trailing PTY Enter on the attach TUI.
        return Ok(());
    }

    // ponytail: ConPTY write_all/flush can block; holding PaneRegistry across
    // that freezes list_panes, keyboard input, and the HTTP bridge.
    let writer = pane.writer.clone();
    drop(reg);

    {
        let mut writer = writer.lock();
        if !text.is_empty() {
            writer
                .write_all(text.as_bytes())
                .map_err(|e| format!("write_all: {e}"))?;
            writer.flush().map_err(|e| format!("flush: {e}"))?;
            // Ink/React TUIs can drop a submitted prompt if the prompt text and
            // Enter arrive in the same instant. Raw terminal input must stay
            // immediate, so only pause for submit-style writes.
            if append_newline {
                thread::sleep(submit_delay(text));
            }
        }
        if append_newline {
            write_enter_bytes(&mut **writer, &agent)?;
        }
    }
    crate::event_log::append_system_event(SystemEvent::PaneInputWritten {
        pane_id: PaneId(pane_id.to_string()),
        byte_count: text.len() + usize::from(append_newline),
        append_newline,
    });
    Ok(())
}

/// Pause between pasted text and Enter. Multi-line or long text is a bracketed paste that the
/// TUI needs time to ingest; an Enter that lands mid-paste is dropped and the prompt stays in
/// the input box unsubmitted.
pub(super) fn submit_delay(text: &str) -> Duration {
    if text.contains('\n') || text.len() > 200 {
        Duration::from_millis((300 + text.len() as u64 / 10).min(1_500))
    } else {
        Duration::from_millis(50)
    }
}

/// True when input should never be routed to OpenCode prompt_async (keys, mouse, escapes).
/// Line breaks and tabs are ordinary prompt text: a multi-line task (acceptance criteria, a
/// context packet) must still go through the API, or it is typed into the attached TUI and lost.
pub(super) fn is_pty_control_input(text: &str) -> bool {
    text.contains('\x1b')
        || text
            .bytes()
            .any(|byte| byte < 32 && !matches!(byte, b'\t' | b'\n' | b'\r'))
}

/// Send Enter to the PTY. Uses \r (xterm/ConPTY).
pub(super) fn write_enter_bytes(writer: &mut dyn Write, _agent_type: &str) -> Result<(), String> {
    writer
        .write_all(b"\r")
        .map_err(|e| format!("write enter: {e}"))?;
    writer.flush().map_err(|e| format!("flush enter: {e}"))?;
    Ok(())
}

pub fn read_buffer(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    lines: usize,
) -> Result<String, String> {
    let scrollback = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        pane.scrollback.clone()
    };
    let text = scrollback.lock().tail_text(lines);
    Ok(strip_ansi(&text))
}

pub fn read_snapshot(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<String, String> {
    let screen = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        (pane.screen.clone(), pane.info.agent_type.clone())
    };
    let (screen, agent_type) = screen;
    let parser = screen.lock();
    let keep = sidebar_keep_cols(&agent_type, parser.screen().size().1);
    Ok(snapshot_text(parser.screen(), keep))
}

/// The screen as text. With `keep_cols` set, one entry per row limited to that many columns
/// (`contents()` would glue a full-width row to the next one and cannot crop).
pub(crate) fn snapshot_text(screen: &vt100::Screen, keep_cols: Option<u16>) -> String {
    match keep_cols {
        Some(keep) => {
            let rows: Vec<String> = screen.rows(0, keep).map(|row| row.trim_end().to_string()).collect();
            rows.join("
")
        }
        None => screen.contents(),
    }
}

/// Columns left after the profile's right sidebar is cropped, `None` when the pane is not wide
/// enough to show one (or the profile has none).
pub(super) fn sidebar_keep_cols(agent_type: &str, cols: u16) -> Option<u16> {
    let crop = crate::tui::profile_for(agent_type).sidebar_crop()?;
    (cols >= crop.min_cols && cols > crop.width).then(|| cols - crop.width)
}

/// The pane's text with its scrollback: up to `FULL_TEXT_MAX_LINES` of history above the visible
/// rows, one entry per row (trailing blanks trimmed). A TUI on the alternate screen has no history,
/// so this is then just the screen.
pub fn read_full_text(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<String, String> {
    let screen = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        (pane.screen.clone(), pane.info.agent_type.clone())
    };
    let (screen, agent_type) = screen;
    let mut parser = screen.lock();
    let keep = sidebar_keep_cols(&agent_type, parser.screen().size().1);
    Ok(full_text_of_cols(&mut parser, keep))
}

pub(super) fn full_text_of(parser: &mut vt100::Parser) -> String {
    full_text_of_cols(parser, None)
}

/// `full_text_of` limited to the first `keep_cols` columns of every row.
pub(super) fn full_text_of_cols(parser: &mut vt100::Parser, keep_cols: Option<u16>) -> String {
    const FULL_TEXT_MAX_LINES: usize = 4000;
    let (_, cols) = parser.screen().size();
    let cols = keep_cols.unwrap_or(cols).min(cols);
    parser.screen_mut().set_scrollback(usize::MAX);
    let total = parser.screen().scrollback().min(FULL_TEXT_MAX_LINES);
    let mut lines: Vec<String> = Vec::new();
    // At offset `n` the first visible row is the n-th line above the live screen.
    for offset in (1..=total).rev() {
        parser.screen_mut().set_scrollback(offset);
        if let Some(row) = parser.screen().rows(0, cols).next() {
            lines.push(row.trim_end().to_string());
        }
    }
    parser.screen_mut().set_scrollback(0);
    lines.extend(
        parser
            .screen()
            .rows(0, cols)
            .map(|row| row.trim_end().to_string()),
    );
    lines.join("\n")
}

#[cfg(test)]
mod full_text_tests {
    use super::full_text_of;

    #[test]
    fn scrollback_and_screen_are_returned_in_order() {
        let mut parser = vt100::Parser::new(10, 40, 1000);
        for n in 1..=60 {
            parser.process(format!("line {n:02}\r\n").as_bytes());
        }
        let full = full_text_of(&mut parser);
        let lines: Vec<&str> = full.lines().filter(|l| l.starts_with("line")).collect();
        assert_eq!(lines.len(), 60, "{full}");
        assert_eq!(lines[0], "line 01");
        assert_eq!(lines[59], "line 60");
        assert_eq!(
            parser.screen().scrollback(),
            0,
            "the view offset is restored"
        );
    }

    #[test]
    fn an_alternate_screen_has_no_history() {
        let mut parser = vt100::Parser::new(5, 40, 1000);
        parser.process(b"history\r\n\r\n\r\n\r\n\r\n\r\n\x1b[?1049hvisible");
        let full = full_text_of(&mut parser);
        assert!(full.contains("visible"));
        assert!(!full.contains("history"), "{full}");
    }
}

pub fn read_raw_buffer(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    lines: usize,
) -> Result<Vec<u8>, String> {
    let scrollback = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        pane.scrollback.clone()
    };
    let raw = scrollback.lock().tail_raw_bytes(lines);
    Ok(raw)
}

pub fn resize(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    cols: u16,
    rows: u16,
) -> Result<bool, String> {
    let (master, screen) = {
        let mut reg = registry.lock();
        let pane = reg
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        if pane.info.cols == cols && pane.info.rows == rows {
            return Ok(false);
        }
        (pane.master.clone(), pane.screen.clone())
    };
    // ponytail: ConPTY resize can block; holding PaneRegistry across it
    // freezes list_panes and keyboard input the same way write_all did.
    master
        .lock()
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize: {e}"))?;
    // Keep the headless emulator's grid the same shape as the PTY so a later
    // screen reconstruction matches the live geometry.
    screen.lock().screen_mut().set_size(rows, cols);
    if let Some(pane) = registry.lock().panes.get_mut(pane_id) {
        pane.info.cols = cols;
        pane.info.rows = rows;
    }
    Ok(true)
}

#[cfg(test)]
mod submit_delay_tests {
    use super::*;

    #[test]
    fn multiline_paste_waits_longer_than_a_short_line() {
        assert_eq!(submit_delay("hi"), Duration::from_millis(50));
        assert!(submit_delay("a\nb") >= Duration::from_millis(300));
        assert_eq!(
            submit_delay(&"x\n".repeat(20_000)),
            Duration::from_millis(1_500)
        );
    }
}
