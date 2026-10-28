//! Telegram's update JSON, mapped to the generic `Update` shape. Pure functions, no network.
//!
//! An item we cannot read is never a reason to stop the poll: an update with an `update_id` always
//! moves the offset on, even when its content is `Other`.

use super::{InCallback, InMessage, Update, UpdateKind};
use serde_json::Value;

/// Maps the `result` array of getUpdates. `None` when the result is not an array.
/// Items without an `update_id` cannot be acknowledged, so they are skipped.
pub fn map_updates(result: &Value) -> Option<Vec<Update>> {
    Some(result.as_array()?.iter().filter_map(map_update).collect())
}

fn map_update(raw: &Value) -> Option<Update> {
    let update_id = raw.get("update_id")?.as_i64()?;
    let kind = match (raw.get("message"), raw.get("callback_query")) {
        (Some(m), _) => map_message(m).map_or(UpdateKind::Other, UpdateKind::Message),
        (None, Some(c)) => map_callback(c).map_or(UpdateKind::Other, UpdateKind::Callback),
        // edited_message, channel_post, my_chat_member, ...: not used in v1.
        _ => UpdateKind::Other,
    };
    Some(Update { update_id, kind })
}

fn map_message(m: &Value) -> Option<InMessage> {
    let chat = m.get("chat")?;
    let from = m.get("from");
    Some(InMessage {
        message_id: m.get("message_id")?.as_i64()?,
        chat_id: chat.get("id")?.as_i64()?,
        chat_private: str_of(chat, "type") == Some("private"),
        from_id: from.and_then(|f| f.get("id")).and_then(Value::as_i64),
        from_is_bot: from.and_then(|f| f.get("is_bot")).and_then(Value::as_bool).unwrap_or(false),
        username: from.and_then(|f| str_of(f, "username")).map(str::to_string),
        first_name: from.and_then(|f| str_of(f, "first_name")).map(str::to_string),
        date_s: m.get("date").and_then(Value::as_i64).unwrap_or(0),
        text: str_of(m, "text").map(str::to_string),
        reply_to: m.get("reply_to_message").and_then(|r| r.get("message_id")).and_then(Value::as_i64),
    })
}

fn map_callback(c: &Value) -> Option<InCallback> {
    let message = c.get("message");
    Some(InCallback {
        id: str_of(c, "id")?.to_string(),
        from_id: c.get("from")?.get("id")?.as_i64()?,
        chat_id: message.and_then(|m| m.get("chat")).and_then(|ch| ch.get("id")).and_then(Value::as_i64),
        message_id: message.and_then(|m| m.get("message_id")).and_then(Value::as_i64),
        data: str_of(c, "data").map(str::to_string),
    })
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one(raw: &str) -> Update {
        let v: Value = serde_json::from_str(raw).unwrap();
        map_update(&v).expect("an update with an update_id")
    }

    #[test]
    fn private_message_maps_to_a_message_from_a_person_in_a_private_chat() {
        let u = one(include_str!("../fixtures/private_message.json"));
        assert_eq!(
            u,
            Update {
                update_id: 1001,
                kind: UpdateKind::Message(InMessage {
                    message_id: 55,
                    chat_id: 1001,
                    chat_private: true,
                    from_id: Some(1001),
                    from_is_bot: false,
                    username: Some("test_user".into()),
                    first_name: Some("Test".into()),
                    date_s: 1761600000,
                    text: Some("/status".into()),
                    reply_to: None,
                }),
            }
        );
    }

    #[test]
    fn group_message_maps_to_a_message_that_is_not_private() {
        let u = one(include_str!("../fixtures/group_message.json"));
        let UpdateKind::Message(m) = u.kind else { panic!("a group message is a Message") };
        assert!(!m.chat_private, "the engine drops non-private chats, so the flag must say so");
        assert_eq!(m.chat_id, -1001234567890);
        assert_eq!(m.from_id, Some(1002));
        assert_eq!(m.reply_to, Some(30));
        assert_eq!(m.text.as_deref(), Some("/panes"));
    }

    #[test]
    fn a_bot_sender_is_flagged_as_a_bot() {
        let raw = json!({"update_id": 7, "message": {"message_id": 1, "date": 1,
            "chat": {"id": 5, "type": "private"}, "from": {"id": 5, "is_bot": true}, "text": "hi"}});
        let UpdateKind::Message(m) = map_update(&raw).unwrap().kind else { panic!("a message") };
        assert!(m.from_is_bot);
        assert_eq!(m.username, None);
    }

    #[test]
    fn callback_maps_to_a_callback_with_its_chat_message_and_data() {
        let u = one(include_str!("../fixtures/callback.json"));
        assert_eq!(
            u,
            Update {
                update_id: 1003,
                kind: UpdateKind::Callback(InCallback {
                    id: "4382".into(),
                    from_id: 1001,
                    chat_id: Some(1001),
                    message_id: Some(77),
                    data: Some("g:AbCdEfGhIjKlMnOpQrStUv".into()),
                }),
            }
        );
    }

    #[test]
    fn inline_callback_without_a_message_has_no_chat() {
        let raw = json!({"update_id": 8, "callback_query": {"id": "9", "from": {"id": 1001},
            "inline_message_id": "abc", "data": "g:x"}});
        let UpdateKind::Callback(c) = map_update(&raw).unwrap().kind else { panic!("a callback") };
        assert_eq!(c.chat_id, None);
        assert_eq!(c.message_id, None);
        assert_eq!(c.data.as_deref(), Some("g:x"));
    }

    #[test]
    fn callback_without_a_sender_is_other() {
        let raw = json!({"update_id": 9, "callback_query": {"id": "9", "data": "g:x"}});
        assert_eq!(map_update(&raw).unwrap().kind, UpdateKind::Other);
    }

    #[test]
    fn edited_message_is_other() {
        let u = one(include_str!("../fixtures/edited_message.json"));
        assert_eq!(u.update_id, 1004);
        assert_eq!(u.kind, UpdateKind::Other);
    }

    #[test]
    fn channel_post_and_other_kinds_are_other() {
        let raw = json!({"update_id": 10, "channel_post": {"message_id": 1, "chat": {"id": -100, "type": "channel"}}});
        assert_eq!(map_update(&raw).unwrap().kind, UpdateKind::Other);
    }

    #[test]
    fn a_message_without_a_chat_is_other_but_still_moves_the_offset() {
        let raw = json!({"update_id": 11, "message": {"message_id": 1, "date": 1}});
        assert_eq!(map_update(&raw), Some(Update { update_id: 11, kind: UpdateKind::Other }));
    }

    #[test]
    fn an_item_without_an_update_id_is_skipped() {
        let result = json!([
            {"message": {"message_id": 1, "chat": {"id": 1, "type": "private"}}},
            {"update_id": 12, "channel_post": {}}
        ]);
        let updates = map_updates(&result).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].update_id, 12);
    }

    #[test]
    fn a_result_that_is_not_an_array_is_none() {
        assert_eq!(map_updates(&json!({"update_id": 1})), None);
        assert_eq!(map_updates(&json!(null)), None);
    }

    #[test]
    fn a_missing_date_is_zero_not_an_error() {
        let raw = json!({"update_id": 13, "message": {"message_id": 2, "chat": {"id": 1, "type": "private"}, "text": "x"}});
        let UpdateKind::Message(m) = map_update(&raw).unwrap().kind else { panic!("a message") };
        assert_eq!(m.date_s, 0);
    }
}
