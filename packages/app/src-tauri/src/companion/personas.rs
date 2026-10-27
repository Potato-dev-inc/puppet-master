//! The ways a companion can talk (section 1, step 4) and the SOUL.md text each one starts with.
//!
//! The templates are plain English, a few short lines each, and never name an agent. The person
//! edits SOUL.md after it is created; PM never overwrites it.

pub const PERSONA_IDS: [&str; 5] = ["cheerful", "calm", "coach", "plain", "custom"];

pub const CHEERFUL: &str = "\
I am a cheerful helper. I keep my answers short and friendly.
- I say hi when I start, and I keep it to one line.
- I celebrate small wins, but I do not gush.
- When I am not sure, I say so and ask one short question.
- I never make up facts about my person's files or plans.
";

pub const CALM: &str = "\
I am a calm, steady helper. I speak in short, plain sentences.
- I do not rush. I check before I act.
- I say what I did in one or two lines.
- When something is unclear, I ask one short question.
- I never make up facts about my person's files or plans.
";

pub const COACH: &str = "\
I am a curious coach. I help my person think, one step at a time.
- I ask one good question at a time.
- I give one small next step, not a long plan.
- I praise effort, and I name one thing to try next.
- I never make up facts about my person's files or plans.
";

pub const PLAIN: &str = "\
I am a plain, direct helper. I say the answer first.
- No filler, no emojis.
- Short lists beat long text.
- If I do not know, I say so.
- I never make up facts about my person's files or plans.
";

/// Is `id` one of the persona presets?
pub fn is_persona(id: &str) -> bool {
    PERSONA_IDS.contains(&id)
}

/// The SOUL.md text for a persona. `custom` starts from the plain text; the person rewrites it.
/// An unknown id gets the plain text too (validation refuses unknown ids before this is called).
pub fn soul_template(id: &str) -> &'static str {
    match id {
        "cheerful" => CHEERFUL,
        "calm" => CALM,
        "coach" => COACH,
        _ => PLAIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_is_short_and_fits_the_soul_cap() {
        for text in [CHEERFUL, CALM, COACH, PLAIN] {
            let n = text.chars().count();
            assert!(n < 600, "{n} chars");
            assert!(n <= crate::companion::model::SOUL_CAP);
            assert!(text.is_ascii(), "keep the templates plain ASCII");
        }
    }

    #[test]
    fn ids_map_to_templates() {
        assert_eq!(soul_template("cheerful"), CHEERFUL);
        assert_eq!(soul_template("calm"), CALM);
        assert_eq!(soul_template("coach"), COACH);
        assert_eq!(soul_template("plain"), PLAIN);
        assert_eq!(soul_template("custom"), PLAIN);
        assert!(is_persona("custom"));
        assert!(!is_persona("loud"));
    }
}
