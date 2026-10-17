//! "Nothing readable is kept": the check that a relay's stored bytes hold no name, branch or note.
//! Shared by the relay tests (`tests_v1`) and the two-person end-to-end test (`fake_tui/team.rs`).

/// The values of `values` that someone holding `stored` could read: in `stored` itself, or in
/// anything a base64 or base64url run in it decodes to (a state slot's `body` is base64 of JSON
/// that holds more base64, so this goes as deep as the text goes).
pub fn readable_in<'a>(stored: &str, values: &[&'a str]) -> Vec<&'a str> {
    let layers = decoded_layers(stored);
    values.iter().copied().filter(|value| layers.iter().any(|layer| holds(layer, value))).collect()
}

/// `stored`, then every printable text that a base64 or base64url run in an earlier layer decodes
/// to. Ciphertext decodes to random bytes, which are not printable UTF-8 (each byte has to be), so
/// it adds nothing a value could turn up in by chance.
pub fn decoded_layers(stored: &str) -> Vec<String> {
    use base64::engine::{general_purpose::GeneralPurpose, DecodePaddingMode, GeneralPurposeConfig};
    use base64::{alphabet, Engine};
    let lenient = GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent).with_decode_allow_trailing_bits(true);
    let engines = [GeneralPurpose::new(&alphabet::URL_SAFE, lenient), GeneralPurpose::new(&alphabet::STANDARD, lenient)];
    let mut layers = vec![stored.to_string()];
    let mut next = 0;
    while next < layers.len() {
        let layer = layers[next].clone();
        next += 1;
        for run in layer.split(|c: char| !(c.is_ascii_alphanumeric() || "-_+/=".contains(c))).filter(|run| run.len() >= 4) {
            for engine in &engines {
                let Some(text) = engine.decode(run).ok().and_then(|bytes| String::from_utf8(bytes).ok()) else { continue };
                if !text.chars().any(|c| c.is_control() && !"\n\r\t".contains(c)) && !layers.contains(&text) {
                    layers.push(text);
                }
            }
        }
    }
    layers
}

/// Whether `text` holds `value` for a person to read. ~5,500 characters of random base64 hold a
/// given three letters by chance about 2% of the time, so a short value made only of base64url
/// characters counts only standing on its own, with no base64url character either side. A value
/// with any other character, or 16 or more characters long, cannot turn up by chance at all.
pub fn holds(text: &str, value: &str) -> bool {
    let base64url = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if value.len() >= 16 || !value.chars().all(base64url) {
        return text.contains(value);
    }
    text.match_indices(value).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(base64url) && !text[at + value.len()..].chars().next().is_some_and(base64url)
    })
}
