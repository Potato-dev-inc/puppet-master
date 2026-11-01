//! Per-bot settings of Potato Bot on disk and in the engine: the talk brain a bot uses, its instructions and
//! their history, the tool switches, the instructions file the brain reads, the chat as Markdown, and what
//! moves aside when a bot is deleted.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::bot_settings::*;
use super::service::BrainState;
use super::service_tests::Rig;
use super::store::{self, BrainKind, FromKind, Message, MsgKind};

/// A helper request from the brain, as the bridge would carry it.
fn helper_request(conversation: &str, task: &str) -> Value {
    json!({"v":1,"type":"helper_request","requestId":"h-off","conversationId":conversation,"task":task})
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-botset-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_defaults_are_the_default_brain_all_three_tools_on_and_no_instructions() {
    let root = temp("defaults");
    let s = load(&root, "nimbus").unwrap();
    assert_eq!(s.brain, None, "Use the default");
    assert_eq!(s.tools, ToolSwitches { helpers: true, computer: true, web: true });
    assert!(s.instructions.is_empty());
    assert_eq!(current_instructions(&s), "");
    assert_eq!(effective_brain(BrainKind::Go, &s), BrainKind::Go, "no choice of its own: the default");
    assert!(!root.join("bot-settings").join("nimbus.json").exists(), "a read writes nothing");
}

#[test]
fn a_bot_id_that_is_not_well_formed_is_refused_and_a_damaged_file_is_never_overwritten() {
    let root = temp("damaged");
    assert_eq!(load(&root, "../escape").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert_eq!(update(&root, "Bad Name", |_| Ok(())).unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    std::fs::create_dir_all(root.join("bot-settings")).unwrap();
    std::fs::write(path(&root, "nimbus").unwrap(), "{ damaged").unwrap();
    assert_eq!(load(&root, "nimbus").unwrap_err().code, "POTATO_STORE_INVALID");
    assert!(update(&root, "nimbus", |s| {
        s.brain = Some(BrainKind::Go);
        Ok(())
    })
    .is_err());
    assert_eq!(std::fs::read_to_string(path(&root, "nimbus").unwrap()).unwrap(), "{ damaged", "the damaged file is left as it is");
}

#[test]
fn a_change_is_written_and_read_back_and_a_second_one_keeps_the_first() {
    let root = temp("roundtrip");
    update(&root, "nimbus", |s| {
        s.brain = Some(BrainKind::Agents);
        Ok(())
    })
    .unwrap();
    update(&root, "nimbus", |s| {
        s.tools.web = false;
        Ok(())
    })
    .unwrap();
    let s = load(&root, "nimbus").unwrap();
    assert_eq!(s.brain, Some(BrainKind::Agents), "the first change survives the second");
    assert!(!s.tools.web && s.tools.helpers && s.tools.computer);
    assert_eq!(effective_brain(BrainKind::Deepseek, &s), BrainKind::Agents);
    let text = std::fs::read_to_string(path(&root, "nimbus").unwrap()).unwrap();
    assert!(text.contains("\"brain\": \"agents\""), "the brain is stored by its plain name: {text}");
}

#[test]
fn every_save_is_a_version_and_the_same_text_adds_none() {
    let mut s = BotSettings::default();
    assert!(save_instructions(&mut s, "Be brief.", 100).unwrap());
    assert!(!save_instructions(&mut s, "  Be brief.  ", 200).unwrap(), "the same text after trimming adds nothing");
    assert!(save_instructions(&mut s, "Be brief and kind.", 300).unwrap());
    assert_eq!(s.instructions.len(), 2);
    assert_eq!(current_instructions(&s), "Be brief and kind.");
    let options = options_of(&s, true);
    assert_eq!(options.versions.iter().map(|v| v.text.as_str()).collect::<Vec<_>>(), ["Be brief and kind.", "Be brief."], "newest first");
    assert!(options.sandbox_on);
}

#[test]
fn clearing_the_text_is_a_version_too_and_nothing_is_lost() {
    let mut s = BotSettings::default();
    save_instructions(&mut s, "Speak French.", 10).unwrap();
    assert!(save_instructions(&mut s, "", 20).unwrap());
    assert_eq!(current_instructions(&s), "");
    assert_eq!(s.instructions.len(), 2, "the old text is still in the history");
}

#[test]
fn restore_adds_the_old_text_as_the_newest_version_and_restoring_the_current_one_does_nothing() {
    let mut s = BotSettings::default();
    save_instructions(&mut s, "first", 1_000).unwrap();
    save_instructions(&mut s, "second", 2_000).unwrap();
    let first_at = s.instructions[0].at;
    assert!(restore_instructions(&mut s, first_at, 3_000).unwrap());
    assert_eq!(current_instructions(&s), "first");
    assert_eq!(s.instructions.len(), 3, "restore is itself a version: the history is kept");
    assert!(!restore_instructions(&mut s, first_at, 4_000).unwrap(), "already the one in use");
    assert_eq!(restore_instructions(&mut s, 999_999, 5_000).unwrap_err().code, "POTATO_VERSION_NOT_FOUND");
}

#[test]
fn versions_are_capped_at_fifty_and_their_keys_stay_unique_within_one_millisecond() {
    let mut s = BotSettings::default();
    for i in 0..(MAX_VERSIONS + 5) {
        save_instructions(&mut s, &format!("version {i}"), 500).unwrap();
    }
    assert_eq!(s.instructions.len(), MAX_VERSIONS, "the oldest go first");
    assert_eq!(current_instructions(&s), format!("version {}", MAX_VERSIONS + 4));
    let mut keys: Vec<i64> = s.instructions.iter().map(|v| v.at).collect();
    let before = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), before, "every save got its own key");
}

#[test]
fn instructions_over_the_limit_are_refused_and_the_limit_counts_what_was_typed() {
    let mut s = BotSettings::default();
    let long = "a".repeat(MAX_INSTRUCTION_CHARS + 1);
    assert_eq!(save_instructions(&mut s, &long, 1).unwrap_err().code, "POTATO_INSTRUCTIONS_TOO_LONG");
    assert!(s.instructions.is_empty());
    let exact = "b".repeat(MAX_INSTRUCTION_CHARS);
    assert!(save_instructions(&mut s, &exact, 2).unwrap());
}

#[test]
fn the_instructions_in_use_are_written_to_the_persona_folder_and_removed_when_cleared() {
    let root = temp("instructions-file");
    let persona = root.join("persona");
    std::fs::create_dir_all(&persona).unwrap();
    let mut s = BotSettings::default();
    save_instructions(&mut s, "Keep answers short.", 1).unwrap();
    write_instructions_file(&persona, &s).unwrap();
    assert_eq!(std::fs::read_to_string(persona.join(INSTRUCTIONS_FILE)).unwrap(), "Keep answers short.");
    save_instructions(&mut s, "", 2).unwrap();
    write_instructions_file(&persona, &s).unwrap();
    assert!(!persona.join(INSTRUCTIONS_FILE).exists(), "a cleared text does not come back at the next start");
}

#[test]
fn the_tool_switches_map_the_sandbox_tools_to_their_switch_and_leave_other_names_alone() {
    let all = ToolSwitches::default();
    for tool in ["computer_observe", "computer_act", "ask_person_for_screen"] {
        assert_eq!(sandbox_tool_area(tool), Some(Area::Computer), "{tool}");
        assert!(sandbox_tool_on(&all, tool));
        assert!(!sandbox_tool_on(&ToolSwitches { computer: false, ..all }, tool), "{tool} is off with the computer switch off");
        assert!(sandbox_tool_on(&ToolSwitches { web: false, ..all }, tool), "{tool} does not need the web switch");
    }
    for tool in ["browser_navigate", "browser_snapshot", "browser_act"] {
        assert_eq!(sandbox_tool_area(tool), Some(Area::Web), "{tool}");
        assert!(!sandbox_tool_on(&ToolSwitches { web: false, ..all }, tool), "{tool} is off with the web switch off");
        assert!(sandbox_tool_on(&ToolSwitches { computer: false, ..all }, tool));
    }
    assert_eq!(sandbox_tool_area("write_file"), None);
    assert!(sandbox_tool_on(&ToolSwitches { computer: false, web: false, helpers: false }, "write_file"), "not ours: the tool layer answers it");
}

#[test]
fn a_name_is_trimmed_and_must_be_one_to_forty_characters() {
    assert_eq!(clean_name("  Nimbus  ").unwrap(), "Nimbus");
    assert_eq!(clean_name("   ").unwrap_err().code, "POTATO_NAME_EMPTY");
    assert_eq!(clean_name(&"n".repeat(41)).unwrap_err().code, "POTATO_NAME_TOO_LONG");
    assert!(clean_name(&"n".repeat(40)).is_ok());
}

#[test]
fn the_brain_names_are_the_words_of_the_settings_list() {
    assert_eq!(brain_name(BrainKind::Deepseek), "DeepSeek");
    assert_eq!(brain_name(BrainKind::Go), "your Go plan");
    assert_eq!(brain_name(BrainKind::Agents), "your agents");
}

fn message(at: i64, from: FromKind, name: Option<&str>, kind: MsgKind, text: &str) -> Message {
    Message::new(at, from, name, kind, text)
}

#[test]
fn the_chat_exports_as_markdown_with_a_title_labelled_turns_and_italic_notes() {
    let messages = vec![
        message(1_000, FromKind::You, None, MsgKind::Text, "hello there"),
        message(2_000, FromKind::Bot, Some("Nimbus"), MsgKind::Text, "Hi!  "),
        message(3_000, FromKind::Note, None, MsgKind::Note, "Now talking as Potato."),
    ];
    let md = chat_markdown("Nimbus", &messages);
    assert!(md.starts_with("# Chat with Nimbus\n\n"), "{md}");
    assert!(md.contains("**You** · "), "{md}");
    assert!(md.contains("\n\nhello there\n\n"), "{md}");
    assert!(md.contains("**Nimbus** · "), "{md}");
    assert!(md.contains("\n\nHi!\n\n"), "the text is trimmed: {md}");
    assert!(md.contains("Now talking as Potato.*\n\n"), "a note is italic: {md}");
    assert!(md.find("hello there").unwrap() < md.find("Hi!").unwrap(), "oldest first");
}

#[test]
fn deleting_moves_the_chat_settings_and_read_mark_aside_and_keeps_the_others() {
    let root = temp("delete-files");
    let chat = store::app_conversation("nimbus");
    store::append_message(&root, &chat, message(1, FromKind::You, None, MsgKind::Text, "hi"), None).unwrap();
    store::mark_read(&root, "nimbus", 5).unwrap();
    store::mark_read(&root, "other", 5).unwrap();
    update(&root, "nimbus", |s| {
        s.brain = Some(BrainKind::Go);
        Ok(())
    })
    .unwrap();

    let to = move_bot_files(&root, "nimbus", "42").unwrap();
    assert!(to.ends_with("nimbus-42-bot"), "{}", to.display());
    assert!(to.join("chat.jsonl").exists() && to.join("settings.json").exists());
    assert!(!path(&root, "nimbus").unwrap().exists());
    assert!(store::read_messages(&root, &chat, 10, None).unwrap().is_empty(), "the chat is gone from the list");
    let reads = store::load_reads(&root).unwrap();
    assert!(!reads.contains_key("nimbus") && reads.contains_key("other"));
}

// ---- the engine, on the fakes -----------------------------------------------------------------------

#[test]
fn a_bot_with_its_own_brain_starts_on_that_brain_and_the_default_stays_for_the_others() {
    let rig = Rig::new("own-brain");
    rig.svc.set_talk_brain("nimbus", Some(BrainKind::Go)).unwrap();
    assert!(rig.notes().iter().any(|n| n == "Now using your Go plan."));
    assert!(rig.svc.send("nimbus", "hi").unwrap().delivered);
    assert_eq!(rig.launcher.spawned.lock()[0].provider, "opencode", "the bot's own brain, not the default DeepSeek");
    assert_eq!(rig.svc.bot_options("nimbus").unwrap().brain, Some(BrainKind::Go));
    assert_eq!(rig.svc.state().unwrap().bots[0].talk_brain, Some(BrainKind::Go), "the page's header names the bot's own brain");
    assert_eq!(rig.svc.settings().unwrap().brain, BrainKind::Deepseek, "the default is untouched");
}

#[test]
fn a_bot_on_through_my_agents_starts_no_process_while_the_default_is_a_brain_and_back_to_default_starts_one() {
    let rig = Rig::new("agents-own");
    rig.svc.set_talk_brain("nimbus", Some(BrainKind::Agents)).unwrap();
    let out = rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!((out.delivered, out.state), (false, BrainState::Agents));
    assert_eq!(rig.spawned(), 0);
    rig.svc.set_talk_brain("nimbus", None).unwrap();
    assert!(rig.notes().iter().any(|n| n == "Now using the default brain."));
    assert!(rig.svc.send("nimbus", "again").unwrap().delivered);
    assert_eq!(rig.spawned(), 1);
}

#[test]
fn the_default_through_my_agents_applies_only_to_bots_without_a_brain_of_their_own() {
    let rig = Rig::new("agents-default");
    rig.svc.set_settings(store::Settings { brain: BrainKind::Agents, ..store::Settings::default() }).unwrap();
    assert_eq!(rig.state(), BrainState::Agents);
    rig.svc.set_talk_brain("nimbus", Some(BrainKind::Deepseek)).unwrap();
    assert_ne!(rig.state(), BrainState::Agents, "its own DeepSeek brain wins over the default");
}

#[test]
fn saved_instructions_reach_the_brains_persona_folder_at_its_next_start() {
    let rig = Rig::ready("instr");
    rig.svc.set_instructions("nimbus", "Always answer with a question.").unwrap();
    let persona = rig.root.join("brain").join("nimbus").join("persona");
    rig.child(0).exit_with(0);
    rig.svc.tick();
    rig.svc.send("nimbus", "start again").unwrap();
    assert_eq!(std::fs::read_to_string(persona.join(INSTRUCTIONS_FILE)).unwrap(), "Always answer with a question.");
    assert!(persona.join("core.md").exists(), "the seeded persona is still there");
}

#[test]
fn restoring_an_old_version_restarts_the_brain_with_that_text() {
    let rig = Rig::ready("restore");
    rig.svc.set_instructions("nimbus", "Use plain words.").unwrap();
    let first = rig.svc.bot_options("nimbus").unwrap().versions[0].at;
    rig.svc.set_instructions("nimbus", "Use long words.").unwrap();
    let saved = rig.svc.restore_instructions("nimbus", first).unwrap();
    assert_eq!(current_instructions(&saved), "Use plain words.");
    assert_eq!(saved.instructions.len(), 3);
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "shutdown"), "a changed text restarts the brain: its persona is fixed at start");
}

#[test]
fn switching_helpers_off_refuses_a_helper_request_with_no_card() {
    let rig = Rig::ready("helpers-off");
    rig.svc.set_tools("nimbus", ToolSwitches { helpers: false, ..ToolSwitches::default() }).unwrap();
    rig.svc.send("nimbus", "work: add a readme").unwrap();
    rig.post(vec![helper_request("app:dm:nimbus", "add a readme")]);
    assert!(rig.thread().iter().all(|m| m.kind != MsgKind::HelperCard), "no card while helpers are off");
    assert_eq!(rig.svc.state().unwrap().bots[0].open_cards, 0);
}

#[test]
fn the_tool_switches_are_saved_and_read_back_with_the_sandbox_switch() {
    let rig = Rig::new("tools-read");
    let options = rig.svc.bot_options("nimbus").unwrap();
    assert!(!options.sandbox_on, "the test rig has no sandbox Labs switch on");
    let saved = rig.svc.set_tools("nimbus", ToolSwitches { computer: false, web: true, helpers: true }).unwrap();
    assert!(!saved.tools.computer);
    assert!(!rig.svc.bot_options("nimbus").unwrap().tools.computer);
}

#[test]
fn deleting_needs_the_typed_name_then_stops_the_brain_and_takes_the_bot_off_the_list() {
    let rig = Rig::ready("delete");
    assert_eq!(rig.svc.delete_bot("nimbus", "nimb").unwrap_err().code, "POTATO_NAME_MISMATCH");
    assert_eq!(rig.svc.state().unwrap().bots.len(), 1, "a wrong name removes nothing");
    rig.svc.delete_bot("nimbus", " Nimbus ").unwrap();
    assert!(!rig.child(0).running(), "its brain is stopped");
    assert!(rig.svc.state().unwrap().bots.is_empty());
    assert!(!rig.root.join("brain").join("nimbus").exists(), "its brain folder is moved aside");
    assert!(!path(&rig.root, "nimbus").unwrap().exists());
    assert_eq!(rig.svc.messages("nimbus", 5).unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    // Recently deleted: the bot waits there for 7 days and comes back with `trash::restore`.
    let listed = crate::potato::trash::list(&rig.root).unwrap();
    assert!(listed.iter().any(|i| i.kind == "bot" && i.bot == "nimbus" && i.title == "Nimbus"), "{listed:?}");
    crate::potato::trash::restore(&rig.root, &listed[0].id).unwrap();
    assert_eq!(rig.svc.state().unwrap().bots.len(), 1, "restored");
}

#[test]
fn the_chat_of_a_bot_exports_as_markdown_from_the_engine() {
    let rig = Rig::ready("export");
    rig.answer_all();
    let md = rig.svc.export_markdown("nimbus").unwrap();
    assert!(md.starts_with("# Chat with Nimbus"), "{md}");
    assert!(md.contains("hello"), "{md}");
}
