//! Flattening a mode's chart into one entry per DMX channel.
//!
//! A chart is written the way a manual prints it, which means it
//! compresses. Three forms turn up in the documents, and all three have
//! to become plain per-channel entries before anything can address a
//! byte:
//!
//! ```text
//! {"channel": "1-24", "attribute": "Dimmer (per LED section)"}
//! {"channel": "1-24", "attribute": "Red/Green/Blue per section"}
//! {"channel": "1-15", "attribute": "as 15ch"}
//! ```
//!
//! The first repeats one function across the span. The second cycles a
//! list of functions across it — twenty-four channels of R, G, B, R, G,
//! B… — which is how a batten with eight sections is charted. The third
//! borrows another mode's chart wholesale, which is how a 33ch mode says
//! "the first fifteen are the 15ch mode".
//!
//! Ported from `expand_channels` in `tools/make_gdtf.py`. The two must
//! agree — see the note at the top of [`crate::attribute`].

use crate::attribute::{Function, Known, letter, resolve};
use crate::document::{Channel, ChannelRef, Mode, Range};
use std::collections::BTreeMap;

/// One DMX channel of a mode, after every compressed form is flattened.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// 1-based channel number within the mode.
    pub number: u16,
    /// What this channel does.
    pub function: Function,
    /// The manual's own name for it, before resolution — kept for the
    /// patch sheet, which should say what the manual says.
    pub name: String,
    pub ranges: Vec<Range>,
    pub default: Option<u8>,
}

/// Something a chart said that could not be flattened.
///
/// Never an error. A chart line nobody can read is a gap in the
/// research, and the rest of the fixture still has to patch — so the
/// unreadable line is reported and the fixture keeps its other
/// channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Complaint {
    pub mode: String,
    pub channel: String,
    pub message: String,
}

/// Flatten one mode.
///
/// `all` is the document's whole mode map, so an `as <mode>` line can
/// find what it refers to. Returns the channels in ascending order with
/// the complaints alongside.
#[must_use]
pub fn mode(name: &str, modes: &BTreeMap<String, Mode>) -> (Vec<Resolved>, Vec<Complaint>) {
    let mut out = Vec::new();
    let mut complaints = Vec::new();
    expand_into(name, modes, &mut out, &mut complaints, 0);
    out.sort_by_key(|r| r.number);
    out.dedup_by_key(|r| r.number);
    (out, complaints)
}

/// How deep an `as <mode>` chain may go before it is called a cycle.
/// Three is generous: a chart referring to a chart referring to a chart
/// is already a document that wants rewriting.
const MAX_DEPTH: u8 = 3;

fn expand_into(
    name: &str,
    modes: &BTreeMap<String, Mode>,
    out: &mut Vec<Resolved>,
    complaints: &mut Vec<Complaint>,
    depth: u8,
) {
    let Some(current) = modes.get(name) else {
        complaints.push(Complaint {
            mode: name.to_owned(),
            channel: String::new(),
            message: "no such mode".to_owned(),
        });
        return;
    };
    for line in &current.channels {
        expand_line(name, line, modes, out, complaints, depth);
    }
}

fn expand_line(
    mode_name: &str,
    line: &Channel,
    modes: &BTreeMap<String, Mode>,
    out: &mut Vec<Resolved>,
    complaints: &mut Vec<Complaint>,
    depth: u8,
) {
    let complain = |complaints: &mut Vec<Complaint>, message: String| {
        complaints.push(Complaint {
            mode: mode_name.to_owned(),
            channel: match line.channel {
                ChannelRef::One(n) => n.to_string(),
                ChannelRef::Span(lo, hi) => format!("{lo}-{hi}"),
            },
            message,
        });
    };

    let ChannelRef::Span(lo, hi) = line.channel else {
        out.push(Resolved {
            number: line.channel.first(),
            function: resolve(&line.attribute),
            name: line.attribute.clone(),
            ranges: line.ranges.clone(),
            default: line.default,
        });
        return;
    };

    // `as 15ch` / `same as 15ch` / `identical to 15ch`: take that mode's
    // chart and keep the part of it inside this span.
    if let Some(referenced) = borrowed_mode(&line.attribute) {
        if depth >= MAX_DEPTH {
            complain(
                complaints,
                format!("`{}` nests too deep to follow", line.attribute),
            );
            return;
        }
        if !modes.contains_key(referenced) {
            complain(
                complaints,
                format!("refers to mode `{referenced}`, which this fixture does not have"),
            );
            return;
        }
        let mut borrowed = Vec::new();
        expand_into(
            referenced,
            modes,
            &mut borrowed,
            complaints,
            depth.saturating_add(1),
        );
        out.extend(
            borrowed
                .into_iter()
                .filter(|r| r.number >= lo && r.number <= hi),
        );
        return;
    }

    // `Red/Green/Blue per section`, `Dimmer (per LED section)`: the
    // functions before the `per` cycle across the span.
    let functions = cycled(&line.attribute);
    if functions.is_empty() {
        complain(
            complaints,
            format!("cannot spread `{}` across the span", line.attribute),
        );
        return;
    }
    let count = line.channel.count();
    let stride = u16::try_from(functions.len()).unwrap_or(u16::MAX);
    if stride == 0 || !count.is_multiple_of(stride) {
        complain(
            complaints,
            format!(
                "{count} channels do not divide into {} functions",
                functions.len()
            ),
        );
        return;
    }
    // Cycling the function list is the whole of the compressed form:
    // twenty-four channels over [R, G, B] is R G B R G B eight times.
    for (step, known) in functions
        .iter()
        .cycle()
        .take(usize::from(count))
        .enumerate()
    {
        let step = u16::try_from(step).unwrap_or(u16::MAX);
        out.push(Resolved {
            number: lo.saturating_add(step),
            function: Function::Known(*known),
            name: known.canonical().to_owned(),
            // Ranges on a compressed line describe the *one* function it
            // repeats. Attaching them to each of R, G and B in turn
            // would claim the red channel's chart says what the blue
            // channel's does.
            ranges: if functions.len() == 1 {
                line.ranges.clone()
            } else {
                Vec::new()
            },
            default: line.default,
        });
    }
}

/// `as 15ch` → `15ch`.
fn borrowed_mode(attribute: &str) -> Option<&str> {
    let text = attribute.trim();
    for prefix in ["as ", "same as ", "identical to "] {
        if let Some(rest) = strip_prefix_ignoring_case(text, prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest);
            }
        }
    }
    None
}

fn strip_prefix_ignoring_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| text.get(prefix.len()..))
        .flatten()
}

/// The list of functions a compressed name cycles through.
///
/// Everything from the start of the name up to `per` / `for each` /
/// `each` is the list; it is split on the separators a manual uses
/// (`/`, `,`, `+`, `&`) and each part resolved on its own. One part is
/// the "repeat this function" case; several is the "cycle these" case.
/// An empty result means the name could not be spread at all.
fn cycled(attribute: &str) -> Vec<Known> {
    let head = ["per ", "for each ", "each "]
        .iter()
        .filter_map(|word| find_word(attribute, word))
        .min()
        .and_then(|at| attribute.get(..at))
        .unwrap_or(attribute);
    let head = head.split('(').next().unwrap_or(head);
    let mut out = Vec::new();
    for part in head.split(['/', ',', '+', '&']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some(known) = letter(part).or_else(|| match resolve(part) {
            Function::Known(known) => Some(known),
            Function::Unknown(_) => None,
        }) else {
            return Vec::new();
        };
        out.push(known);
    }
    out
}

/// The byte offset of `needle` in `haystack`, case-insensitively, only
/// where it starts on a word boundary.
fn find_word(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    let mut from = 0_usize;
    while let Some(at) = lower.get(from..).and_then(|rest| rest.find(needle)) {
        let at = from.saturating_add(at);
        let before_is_boundary = at == 0
            || lower
                .get(..at)
                .and_then(|head| head.chars().next_back())
                .is_none_or(|c| !c.is_alphanumeric());
        if before_is_boundary {
            return Some(at);
        }
        from = at.saturating_add(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Resolved, mode};
    use crate::attribute::{Function, Known};
    use crate::document::{FixtureType, Mode};
    use std::collections::BTreeMap;

    fn modes(json: &str) -> BTreeMap<String, Mode> {
        let doc: FixtureType =
            serde_json::from_str(&format!(r#"{{"console_name": "T", "modes": {json}}}"#)).unwrap();
        doc.modes
    }

    fn functions(rows: &[Resolved]) -> Vec<Function> {
        rows.iter().map(|r| r.function.clone()).collect()
    }

    #[test]
    fn one_function_repeats_across_a_span() {
        let (rows, complaints) = mode(
            "24ch",
            &modes(r#"{"24ch": [{"channel": "1-4", "attribute": "Dimmer (per LED section)"}]}"#),
        );
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(rows.len(), 4);
        assert!(
            rows.iter()
                .all(|r| r.function == Function::Known(Known::Dimmer))
        );
        assert_eq!(
            rows.iter().map(|r| r.number).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn a_list_of_functions_cycles_across_a_span() {
        // Six channels of a three-section RGB batten: R G B R G B.
        let (rows, complaints) = mode(
            "6ch",
            &modes(r#"{"6ch": [{"channel": "1-6", "attribute": "Red/Green/Blue per section"}]}"#),
        );
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(
            functions(&rows),
            [
                Function::Known(Known::Red),
                Function::Known(Known::Green),
                Function::Known(Known::Blue),
                Function::Known(Known::Red),
                Function::Known(Known::Green),
                Function::Known(Known::Blue),
            ]
        );
    }

    #[test]
    fn a_span_that_does_not_divide_is_reported_not_guessed() {
        // Five channels into three functions is a chart that is wrong.
        // Splitting it anyway would put green where blue is on every
        // second section, which is worse than saying so.
        let (rows, complaints) = mode(
            "5ch",
            &modes(r#"{"5ch": [{"channel": "1-5", "attribute": "Red/Green/Blue per section"}]}"#),
        );
        assert!(rows.is_empty());
        assert_eq!(complaints.len(), 1);
        assert!(
            complaints[0].message.contains("do not divide"),
            "{complaints:?}"
        );
    }

    #[test]
    fn a_mode_can_borrow_another_modes_chart() {
        // The ZQ01334's 33ch mode opens with its 15ch mode verbatim.
        let (rows, complaints) = mode(
            "5ch",
            &modes(
                r#"{
                  "3ch": [{"channel": 1, "attribute": "Red"},
                          {"channel": 2, "attribute": "Green"},
                          {"channel": 3, "attribute": "Blue"}],
                  "5ch": [{"channel": "1-3", "attribute": "as 3ch"},
                          {"channel": 4, "attribute": "Dimmer"},
                          {"channel": 5, "attribute": "Strobe"}]
                }"#,
            ),
        );
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(
            functions(&rows),
            [
                Function::Known(Known::Red),
                Function::Known(Known::Green),
                Function::Known(Known::Blue),
                Function::Known(Known::Dimmer),
                Function::Known(Known::Strobe),
            ]
        );
    }

    #[test]
    fn a_reference_to_a_missing_mode_complains_and_keeps_the_rest() {
        let (rows, complaints) = mode(
            "5ch",
            &modes(
                r#"{"5ch": [{"channel": "1-3", "attribute": "as 3ch"},
                            {"channel": 4, "attribute": "Dimmer"}]}"#,
            ),
        );
        assert_eq!(rows.len(), 1, "the readable line survives");
        assert_eq!(complaints.len(), 1);
        assert!(
            complaints[0].message.contains("does not have"),
            "{complaints:?}"
        );
    }
}
