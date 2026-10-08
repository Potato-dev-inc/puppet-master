use super::KILO;
use crate::tui::declarative::schema::Definition;
use crate::tui::declarative::template::{render, render_stdin};
use crate::tui::declarative::parse_definition;
use crate::tui::types::HeadlessLaunch;

fn kilo() -> Definition {
    parse_definition(KILO).unwrap()
}

fn launch<'a>(task: &'a str) -> HeadlessLaunch<'a> {
    HeadlessLaunch::new("C:\\proj", task)
}

fn kilo_argv(launch: &HeadlessLaunch<'_>) -> Vec<String> {
    render(kilo().headless.as_ref().unwrap(), launch)
}

#[test]
fn plain_launch_is_base_argv_separator_task() {
    assert_eq!(kilo_argv(&launch("fix it")), ["run", "--format", "json", "--", "fix it"]);
}

#[test]
fn when_segments_in_order() {
    let mut l = launch("do work");
    l.model = Some("kilo/kilo-auto/free");
    l.resume_session = Some("ses_1");
    l.allow_shell = true;
    // declaration order is model, resume, allow_shell; the task trails after the separator
    assert_eq!(
        kilo_argv(&l),
        ["run", "--format", "json", "-m", "kilo/kilo-auto/free", "-s", "ses_1", "--auto", "--", "do work"]
    );
    // each segment is independent
    let mut l = launch("t");
    l.allow_shell = true;
    assert_eq!(kilo_argv(&l), ["run", "--format", "json", "--auto", "--", "t"]);
    let mut l = launch("t");
    l.resume_session = Some("s");
    assert_eq!(kilo_argv(&l), ["run", "--format", "json", "-s", "s", "--", "t"]);
}

#[test]
fn read_only_and_trust_segments() {
    let mut def = kilo();
    {
        let h = def.headless.as_mut().unwrap();
        h.optional.push(crate::tui::declarative::schema::OptionalSegment {
            when: "read_only".into(),
            args: vec!["--agent".into(), "plan".into()],
        });
        h.optional.push(crate::tui::declarative::schema::OptionalSegment {
            when: "not_read_only".into(),
            args: vec!["--write".into()],
        });
        h.optional.push(crate::tui::declarative::schema::OptionalSegment {
            when: "trust_workspace".into(),
            args: vec!["--trust".into()],
        });
    }
    let h = def.headless.as_ref().unwrap();
    let mut l = launch("t");
    assert_eq!(render(h, &l), ["run", "--format", "json", "--write", "--", "t"]);
    l.read_only = true;
    l.trust_workspace = true;
    assert_eq!(render(h, &l), ["run", "--format", "json", "--agent", "plan", "--trust", "--", "t"]);
}

#[test]
fn task_flag_and_stdin_and_explicit_placeholder() {
    let mut def = kilo();
    def.headless.as_mut().unwrap().task_flag = Some("--prompt".into());
    let h = def.headless.as_ref().unwrap();
    assert_eq!(render(h, &launch("x y")), ["run", "--format", "json", "--prompt", "x y"]);

    let mut def = kilo();
    let h = def.headless.as_mut().unwrap();
    h.stdin = Some(crate::tui::declarative::schema::StdinTable { template: "{task}\n".into() });
    let h = def.headless.as_ref().unwrap();
    assert_eq!(render(h, &launch("x y")), ["run", "--format", "json"]);
    assert_eq!(render_stdin(h, &launch("x y")).as_deref(), Some("x y\n"));
    assert_eq!(render_stdin(kilo().headless.as_ref().unwrap(), &launch("t")), None);

    let mut def = kilo();
    let h = def.headless.as_mut().unwrap();
    h.argv = vec!["ask".into(), "--text={task}".into(), "--cwd".into(), "{project}".into()];
    h.task_separator = None;
    let h = def.headless.as_ref().unwrap();
    assert_eq!(render(h, &launch("hi")), ["ask", "--text=hi", "--cwd", "C:\\proj"]);
}

#[test]
fn missing_values_omit_the_element() {
    let mut def = kilo();
    def.headless.as_mut().unwrap().argv = vec!["run".into(), "{model}".into(), "{reasoning}".into(), "{session}".into()];
    let h = def.headless.as_ref().unwrap();
    assert_eq!(render(h, &launch("t")), ["run", "--", "t"]);
}

/// Deterministic pseudo-random generator so the "property" test needs no extra crate.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn hostile_tasks() -> Vec<String> {
    let mut tasks: Vec<String> = [
        "", " ", "with spaces and tabs\t.", "\"double\" 'single' `tick`", "a; rm -rf / ; b", "x && y || z | w > f < g",
        "line1\nline2\r\nline3", "--leading-dash", "-", "--", "-m evil/model", "{model} {session} {task} {project} {nope}",
        "%PATH% $HOME $(whoami) `id`", "\\ \\\\ \"\\\"", "\0nul", "caf\u{e9} \u{1f600} \u{202e}rtl", "{", "}", "{{task}}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    tasks.push("A".repeat(100 * 1024));
    tasks.push(format!("--{}", "-x ".repeat(40_000)));
    let alphabet: Vec<char> = " \t\n\r\"'`;&|<>$%{}\\-ab9".chars().collect();
    let mut rng = Lcg(0x5eed);
    for _ in 0..200 {
        let len = (rng.next() % 300) as usize;
        tasks.push((0..len).map(|_| alphabet[(rng.next() as usize) % alphabet.len()]).collect());
    }
    tasks
}

#[test]
fn task_is_one_argv_element() {
    let base = {
        let mut l = launch("baseline");
        l.model = Some("m/x");
        l.resume_session = Some("s1");
        l.allow_shell = true;
        kilo_argv(&l)
    };
    let task_index = base.len() - 1;
    assert_eq!(base[task_index], "baseline");
    for task in hostile_tasks() {
        let mut l = launch(&task);
        l.model = Some("m/x");
        l.resume_session = Some("s1");
        l.allow_shell = true;
        let argv = kilo_argv(&l);
        assert_eq!(argv.len(), base.len(), "task changed the element count");
        assert_eq!(argv[..task_index], base[..task_index], "task changed another element");
        assert_eq!(argv[task_index], task, "task was altered");
        assert_eq!(argv[task_index - 1], "--", "separator sits immediately before the task");
    }
}

#[test]
fn task_is_one_element_with_flag_and_placeholder_delivery() {
    for task in hostile_tasks() {
        let mut def = kilo();
        def.headless.as_mut().unwrap().task_flag = Some("--prompt".into());
        let argv = render(def.headless.as_ref().unwrap(), &launch(&task));
        assert_eq!(argv, ["run", "--format", "json", "--prompt", task.as_str()]);

        let mut def = kilo();
        let h = def.headless.as_mut().unwrap();
        h.argv = vec!["ask".into(), "{task}".into(), "--cwd".into(), "{project}".into()];
        let argv = render(def.headless.as_ref().unwrap(), &launch(&task));
        assert_eq!(argv, ["ask", task.as_str(), "--cwd", "C:\\proj"]);
    }
}

#[test]
fn substituted_values_are_not_rescanned() {
    let mut def = kilo();
    def.headless.as_mut().unwrap().argv = vec!["--m={model}".into(), "{project}".into()];
    let mut l = HeadlessLaunch::new("{model}", "t");
    l.model = Some("{session}");
    l.resume_session = Some("S");
    let argv = render(def.headless.as_ref().unwrap(), &l);
    assert_eq!(argv, ["--m={session}", "{model}", "-m", "{session}", "-s", "S", "--", "t"]);
}
