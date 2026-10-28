//! The saved page's style: one grey scale, light by default and dark when the system asks for it.

pub const CSS: &str = r#":root{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--muted:#6e6e6e;--line:#e3e3e3;--raised:#f2f2f2;--strong:#000}
@media (prefers-color-scheme:dark){:root{--bg:#121212;--fg:#ececec;--muted:#9a9a9a;--line:#2c2c2c;--raised:#1f1f1f;--strong:#fff}}
*{box-sizing:border-box}
html,body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif}
.page{max-width:760px;margin:0 auto;padding:24px 16px 32px}
.head{border-bottom:1px solid var(--line);padding-bottom:12px;margin-bottom:18px}
.eyebrow{margin:0;font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}
h1{margin:2px 0 4px;font-size:20px;font-weight:600;color:var(--strong);overflow-wrap:anywhere}
h3{margin:0 0 2px;font-size:11px;font-weight:600;color:var(--muted);text-transform:uppercase;letter-spacing:.06em}
.meta{margin:0;display:flex;flex-wrap:wrap;gap:2px 12px;color:var(--muted);font-size:12px}
.range{margin:6px 0 0;font-size:12px;color:var(--muted)}
.chat{display:flex;flex-direction:column;gap:14px}
.text{white-space:pre-wrap;overflow-wrap:anywhere}
.msg.you .text{background:var(--raised);border-radius:6px;padding:6px 10px}
.tool{font:12px/1.5 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.tool .name{color:var(--fg);font-weight:600}
pre{margin:4px 0 0;font:12px/1.45 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;white-space:pre-wrap;overflow-wrap:anywhere;background:var(--raised);border-radius:6px;padding:8px 10px}
.diff .del{display:block;color:var(--muted);text-decoration:line-through}
.diff .add{display:block;font-weight:600}
.output{color:var(--muted)}
.output.failed{color:var(--fg);border-left:2px solid var(--fg)}
.more{margin:2px 0 0;font-size:11px;color:var(--muted)}
.turn-end{display:flex;align-items:center;gap:8px;font-size:11px;color:var(--muted)}
.turn-end::before,.turn-end::after{content:"";flex:1;height:1px;background:var(--line)}
.plan ul{list-style:none;margin:4px 0 0;padding:0;font-size:13px}
.plan li{padding:1px 0;overflow-wrap:anywhere}
.plan li.done{color:var(--muted);text-decoration:line-through}
.plan .mark{font:12px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;color:var(--muted)}
.notice{margin:0;font-size:12px;font-style:italic;color:var(--muted);overflow-wrap:anywhere}
.empty{color:var(--muted)}
.foot{margin-top:28px;border-top:1px solid var(--line);padding-top:10px;font-size:11px;color:var(--muted)}"#;
