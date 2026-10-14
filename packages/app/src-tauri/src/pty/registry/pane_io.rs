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

    // Auto-title: the first submitted line of an agent pane becomes its title (`ambient::title`).
    // The screen's last line tells whether the line answers a secret prompt; shells are not titled.
    let screen_line = if crate::tui::is_agent_backend(&pane.info.agent_type) {
        crate::ambient::title::last_line(&pane.screen.lock().screen().contents()).to_string()
    } else {
        String::new()
    };
    let titled = crate::ambient::store::observe_input(pane_id, &pane.info.agent_type, &screen_line, text, append_newline);

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
        announce_title(app, pane_id, titled);
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
    announce_title(app, pane_id, titled);

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

/// A title was just set: tell the windows and the tray (never while the registry lock is held).
fn announce_title(app: &AppHandle, pane_id: &str, titled: bool) {
    if titled {
        crate::ambient::announce_title_change(app, pane_id);
    }
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

/// The whole screen, never cropped. Prompt delivery reads this: the sidebar crop assumes a wide
/// pane shows a sidebar, but a TUI's home screen (no session yet) is centred over the full width,
/// so cropping cut the input box and the typed prompt looked missing.
pub fn read_snapshot_full(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<String, String> {
    let screen = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        pane.screen.clone()
    };
    let parser = screen.lock();
    Ok(snapshot_text(parser.screen(), None))
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
    if keep.is_none() {
        if let Some(text) = side_panel_snapshot(parser.screen(), &agent_type) {
            return Ok(text);
        }
    }
    Ok(snapshot_text(parser.screen(), keep))
}

/// The screen one entry per row with a side panel cropped off, for a profile that can show one
/// and a screen that does. `contents()` would glue each full-width chat+panel row to the next.
pub(crate) fn side_panel_snapshot(screen: &vt100::Screen, agent_type: &str) -> Option<String> {
    if !crate::tui::has_side_panel(agent_type) {
        return None;
    }
    let (_, cols) = screen.size();
    let rows: Vec<String> = screen.rows(0, cols).collect();
    crate::tui::side_panel::crop_rows(&rows).map(|rows| rows.join("\n"))
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

/// Columns to keep per row for a profile that declares a right sidebar: the columns left after
/// the sidebar is cropped, or every column when the pane is too narrow to show one (a narrow grid
/// tile). Either way the text is one entry per row, because `contents()` glues a full-width row
/// to the next one and its reply parsing then fails. `None` when the profile has no sidebar.
pub(super) fn sidebar_keep_cols(agent_type: &str, cols: u16) -> Option<u16> {
    keep_cols_for(crate::tui::profile_for(agent_type).sidebar_crop()?, cols)
}

fn keep_cols_for(crop: crate::tui::SidebarCrop, cols: u16) -> Option<u16> {
    if cols >= crop.min_cols && cols > crop.width {
        Some(cols - crop.width)
    } else {
        Some(cols)
    }
}

#[cfg(test)]
mod sidebar_keep_tests {
    use super::keep_cols_for;
    use crate::tui::SidebarCrop;

    #[test]
    fn narrow_panes_keep_every_column_and_wide_ones_crop_the_sidebar() {
        let crop = || SidebarCrop { min_cols: 121, width: 42 };
        assert_eq!(keep_cols_for(crop(), 121), Some(79));
        assert_eq!(keep_cols_for(crop(), 101), Some(101));
        assert_eq!(keep_cols_for(crop(), 48), Some(48));
    }
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
    let full = full_text_of_cols(&mut parser, keep);
    if keep.is_none() && crate::tui::has_side_panel(&agent_type) {
        let rows: Vec<&str> = full.split('\n').collect();
        if let Some(cropped) = crate::tui::side_panel::crop_rows(&rows) {
            return Ok(cropped.join("\n"));
        }
    }
    Ok(full)
}

/// The pane redrawn for a viewer: its scrollback (formatted, at the current width) followed by the
/// screen and input modes, plus the grid size it was drawn for. A phone replays this instead of the
/// raw byte history, which was written for whatever sizes the pane had back then and came out
/// wrapped and overlapping when replayed at the phone's size.
pub fn read_screen_replay(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Result<(Vec<u8>, u16, u16), String> {
    read_screen_replay_at(registry, pane_id).map(|(bytes, cols, rows, _)| (bytes, cols, rows))
}

/// `read_screen_replay` plus the stream offset the snapshot was taken at (`pty::stream_pos`): every
/// byte before it is drawn in the replay, so a viewer writes only live bytes from there on.
pub fn read_screen_replay_at(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
) -> Result<(Vec<u8>, u16, u16, Option<u64>), String> {
    let screen = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        pane.screen.clone()
    };
    let mut parser = screen.lock();
    let offset = crate::pty::stream_pos::get(pane_id)
        .map(|pos| pos.processed.load(std::sync::atomic::Ordering::SeqCst));
    let (rows, cols) = parser.screen().size();
    Ok((screen_replay_of(&mut parser, 3000), cols, rows, offset))
}

pub(super) fn screen_replay_of(parser: &mut vt100::Parser, max_history: usize) -> Vec<u8> {
    let (rows, cols) = parser.screen().size();
    let mut out = Vec::new();
    if parser.screen().alternate_screen() {
        out.extend_from_slice(b"\x1b[?1049h");
    } else {
        parser.screen_mut().set_scrollback(usize::MAX);
        let total = parser.screen().scrollback().min(max_history);
        // At offset `n` the first visible row is the n-th line above the live screen.
        for offset in (1..=total).rev() {
            parser.screen_mut().set_scrollback(offset);
            if let Some(row) = parser.screen().rows_formatted(0, cols).next() {
                out.extend_from_slice(b"\x1b[m");
                out.extend_from_slice(&row);
                out.extend_from_slice(b"\x1b[m\r\n");
            }
        }
        parser.screen_mut().set_scrollback(0);
        // Scroll the history above the viewer's screen before the screen is drawn over it.
        if total > 0 {
            for _ in 1..rows {
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(&parser.screen().state_formatted());
    out
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
    /// Live bug #6: Claude's diff panel was read as part of the chat. The screen is the layout of
    /// `03b-claude-done.png` (164x50, divider at column 90), painted the way the TUI does.
    #[test]
    fn a_side_panel_is_cropped_and_rows_are_not_glued() {
        let fixture = include_str!("../../tui/claude/fixtures/side_diff_panel_screen.txt");
        let mut parser = vt100::Parser::new(50, 164, 0);
        for row in fixture.lines() {
            // A full-width row leaves the cursor in the wrap position, as in the live TUI.
            parser.process(row.as_bytes());
            if row.chars().count() < 164 {
                parser.process(b"\r\n");
            }
        }
        let glued = parser.screen().contents();
        assert!(glued.lines().count() < 48, "the test needs the glue behaviour of contents()");
        let text = super::side_panel_snapshot(parser.screen(), "claude").expect("panel found");
        assert_eq!(text.split('\n').count(), 50, "one entry per row");
        assert!(!text.contains("source: Current") && !text.contains("[ ask ]"), "{text}");
        assert!(text.contains("\u{25cf} Update(sum.js)"), "{text}");
        // Another agent's screen is read as before.
        assert!(super::side_panel_snapshot(parser.screen(), "codex").is_none());
    }

    #[test]
    fn screen_replay_redraws_history_and_screen_at_the_current_size() {
        let mut parser = vt100::Parser::new(5, 40, 1000);
        for n in 0..12 {
            parser.process(format!("line {n:02}\r\n").as_bytes());
        }
        let replay = super::screen_replay_of(&mut parser, 1000);
        let mut viewer = vt100::Parser::new(5, 40, 1000);
        viewer.process(&replay);
        assert_eq!(viewer.screen().contents(), parser.screen().contents());
        viewer.screen_mut().set_scrollback(usize::MAX);
        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(viewer.screen().scrollback(), parser.screen().scrollback());
    }

    #[test]
    fn screen_replay_of_a_tui_enters_the_alternate_screen() {
        let mut parser = vt100::Parser::new(5, 40, 1000);
        parser.process(b"history\r\n\x1b[?1049hvisible");
        let replay = super::screen_replay_of(&mut parser, 1000);
        assert!(replay.starts_with(b"\x1b[?1049h"));
        let mut viewer = vt100::Parser::new(5, 40, 1000);
        viewer.process(&replay);
        assert!(viewer.screen().alternate_screen());
        assert!(viewer.screen().contents().contains("visible"));
    }

    #[test]
    fn wipe_history_keeps_the_screen_and_drops_the_scrollback() {
        let mut parser = vt100::Parser::new(5, 40, 1000);
        for n in 1..=30 {
            parser.process(format!("line {n:02}\r\n").as_bytes());
        }
        parser.process(b"prompt> ");
        let screen_before = parser.screen().contents();
        let cursor = parser.screen().cursor_position();
        super::wipe_history(&mut parser);
        assert_eq!(parser.screen().contents(), screen_before);
        assert_eq!(parser.screen().cursor_position(), cursor);
        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(parser.screen().scrollback(), 0);
        let replay = super::screen_replay_of(&mut parser, 1000);
        assert!(!String::from_utf8_lossy(&replay).contains("line 01"));
    }

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

/// Drops the emulator's scrollback and keeps the screen, cursor and modes.
pub(super) fn wipe_history(parser: &mut vt100::Parser) {
    let (rows, cols) = parser.screen().size();
    let state = parser.screen().state_formatted();
    let mut fresh = vt100::Parser::new(rows, cols, SCROLLBACK_CAP);
    fresh.process(&state);
    *parser = fresh;
}

pub fn resize(
    registry: &Mutex<PaneRegistry>,
    pane_id: &str,
    cols: u16,
    rows: u16,
) -> Result<bool, String> {
    let (master, screen, reprints) = {
        let mut reg = registry.lock();
        let pane = reg
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| unknown_pane_message(pane_id))?;
        if pane.info.cols == cols && pane.info.rows == rows {
            return Ok(false);
        }
        let reprints = crate::tui::profile_for(&pane.info.agent_type).reprints_on_resize();
        (pane.master.clone(), pane.screen.clone(), reprints)
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
    {
        let mut parser = screen.lock();
        parser.screen_mut().set_size(rows, cols);
        // The agent reprints its whole conversation after this resize: keeping the history it
        // already drew would replay a second copy to a viewer that mounts later (viewers wipe
        // their own scrollback at the resize point, see `terminal/reprint-wipe.ts`).
        if reprints {
            wipe_history(&mut parser);
        }
    }
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
