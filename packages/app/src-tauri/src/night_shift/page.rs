//! Night Shift: the saved morning page (SPEC n32 3.8.1). Pure: no I/O, no clock.
//!
//! One self-contained HTML file: the summary line and one block per task. Every string is redacted,
//! has its paths cut to their last names and is HTML-escaped. The page has no copy paths, no pane ids,
//! no links and no script. Saving it is done by the caller through `export_save::check_dest`.

use crate::redact::RedactContext;
use crate::session_chat::export_text::{escape, folder_of};

use super::model::{NightRun, Task};
use super::report;
use super::view::{clean, title_of, turn_text, MAX_CHANGE_LINES, MAX_SUMMARIES};

/// The page's own grey-scale style. `session_chat::export_style` is private to that module, so the
/// same look is written out here. Light by default, dark when the system asks for it.
const CSS: &str = r#":root{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--muted:#6e6e6e;--line:#e3e3e3;--strong:#000}
@media (prefers-color-scheme:dark){:root{--bg:#121212;--fg:#ececec;--muted:#9a9a9a;--line:#2c2c2c;--strong:#fff}}
*{box-sizing:border-box}
html,body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif}
.page{max-width:760px;margin:0 auto;padding:24px 16px 32px}
.head{border-bottom:1px solid var(--line);padding-bottom:12px;margin-bottom:18px}
.eyebrow{margin:0;font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}
h1{margin:2px 0 4px;font-size:20px;font-weight:600;color:var(--strong)}
h2{margin:0;font-size:15px;font-weight:600;color:var(--strong);overflow-wrap:anywhere}
.summary{margin:6px 0 0}
.task{border-bottom:1px solid var(--line);padding:12px 0}
.meta{margin:2px 0 0;display:flex;flex-wrap:wrap;gap:2px 12px;font-size:12px;color:var(--muted)}
.headline{margin:6px 0 0}
.reason,.tests,.files{margin:2px 0 0;font-size:12px;color:var(--muted)}
ul{margin:6px 0 0;padding-left:18px;font-size:13px}
li{overflow-wrap:anywhere;padding:1px 0}
.foot{margin-top:18px;font-size:11px;color:var(--muted)}"#;

const FOOTER: &str = "Saved from Puppet Master";

/// The saved morning page for a run.
pub fn page(run: &NightRun, ctx: &RedactContext) -> String {
    let line = report::line(&report::summary(run));
    let blocks: String = run.tasks.iter().map(|task| task_block(task, ctx)).collect();
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<title>Night Shift morning</title>\n<style>\n{CSS}\n</style>\n</head>\n<body>\n<main class=\"page\">\n<header class=\"head\">\n<p class=\"eyebrow\">Night Shift</p>\n<h1>Morning</h1>\n<p class=\"summary\">{}</p>\n</header>\n<div class=\"tasks\">\n{blocks}</div>\n<footer class=\"foot\">{FOOTER}</footer>\n</main>\n</body>\n</html>\n",
        escape(&line)
    )
}

/// One task: its title, plan, headline, tests, files and what it said. Missing parts are left out.
fn task_block(task: &Task, ctx: &RedactContext) -> String {
    let mut out = format!("<section class=\"task\">\n<h2>{}</h2>\n", escape(&title_of(&task.spec.text, ctx)));

    let mut meta = Vec::new();
    let project = clean(&folder_of(&task.spec.project, ctx.home.as_deref()), ctx);
    if !project.is_empty() {
        meta.push(format!("<span>{}</span>", escape(&project)));
    }
    if let Some(attempt) = task.attempts.last() {
        meta.push(format!("<span>{}</span>", escape(&clean(&attempt.label, ctx))));
    }
    if !meta.is_empty() {
        out.push_str(&format!("<p class=\"meta\">{}</p>\n", meta.concat()));
    }

    match &task.outcome {
        Some(outcome) => {
            out.push_str(&format!("<p class=\"headline\">{}</p>\n", escape(&clean(&outcome.headline, ctx))));
            if let Some(reason) = &outcome.reason {
                out.push_str(&format!("<p class=\"reason\">{}</p>\n", escape(&clean(reason, ctx))));
            }
            if let Some(tests) = outcome.tests {
                let before = outcome
                    .tests_before
                    .map(|b| format!(" (was {} of {})", b.passed, b.total))
                    .unwrap_or_default();
                out.push_str(&format!("<p class=\"tests\">Tests: {} of {} pass{before}</p>\n", tests.passed, tests.total));
            }
            if let Some((count, added, removed)) = outcome.files {
                out.push_str(&format!("<p class=\"files\">{count} files · +{added} -{removed}</p>\n"));
            }
            if !outcome.summaries.is_empty() {
                out.push_str("<ul class=\"turns\">\n");
                for sentence in outcome.summaries.iter().take(MAX_SUMMARIES) {
                    out.push_str(&format!("<li>{}</li>\n", escape(&turn_text(sentence, ctx))));
                }
                out.push_str("</ul>\n");
            }
            if !outcome.test_change_lines.is_empty() {
                out.push_str("<ul class=\"changes\">\n");
                for line in outcome.test_change_lines.iter().take(MAX_CHANGE_LINES) {
                    out.push_str(&format!("<li>{}</li>\n", escape(&clean(line, ctx))));
                }
                out.push_str("</ul>\n");
            }
        }
        None => out.push_str("<p class=\"headline\">Not finished yet.</p>\n"),
    }

    out.push_str("</section>\n");
    out
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
