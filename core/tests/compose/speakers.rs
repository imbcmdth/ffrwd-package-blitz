//! Who is speaking: a caption names its speaker from the document, as the
//! document stands at the tick.

use compose_core::cues::{CueParams, Cues};

use crate::common::*;

const BODY: &str = r#"<p id="captions"></p>
<p data-slot="caster1.name"></p><p data-slot="caster1.twitter">@mango</p>
<p data-slot="caster2.name">  Big
   Ludwig </p><p data-slot="caster2.twitter">@ludwig</p>"#;

const SPEAKERS: &str = r#"[data-slot="caster{n}.name"], [data-slot="caster{n}.twitter"]"#;

fn speaker(n: usize) -> String {
    SPEAKERS.replace("{n}", &n.to_string())
}

#[test]
fn the_text_read_is_the_first_matching_element_with_any_at_the_tick() {
    let mut s = session(&doc("", BODY), 64, 64, 1);
    assert_eq!(
        s.text_at(0.0, &speaker(1)).as_deref(),
        Some("@mango"),
        "an empty name gives way to the handle"
    );
    assert_eq!(
        s.text_at(0.0, &speaker(2)).as_deref(),
        Some("Big Ludwig"),
        "white space is made single spaces"
    );
    assert_eq!(s.text_at(0.0, "#nothing"), None);
    assert_eq!(
        s.text_at(0.0, "[[["),
        None,
        "a selector that does not parse"
    );

    s.fold(
        0.5,
        &lines(&[r#"{"at": 1.0, "select": "[data-slot=\"caster1.name\"]", "text": "Mango"}"#]),
    );
    assert_eq!(s.text_at(0.9, &speaker(1)).as_deref(), Some("@mango"));
    assert_eq!(
        s.text_at(1.0, &speaker(1)).as_deref(),
        Some("Mango"),
        "a row due by the tick is applied before the read"
    );
    render(&mut s, 64, 64, 1.0, 30);
    assert_eq!(
        s.last.rows, 1,
        "the frame counts the row applied for the read"
    );
}

#[test]
fn one_display_names_each_caster_from_the_document() {
    let mut s = session(&doc("", BODY), 64, 64, 1);
    let mut cues = Cues::new("#captions", SPEAKERS, 2, CueParams::default());
    let cue = |text: &str, start: f64| {
        serde_json::json!({"text": text, "start_t": start, "final": false}).to_string()
    };
    cues.fold(0, 0.5, &cue("he goes for the flank", 0.0))
        .unwrap();
    cues.fold(1, 0.6, &cue("what a read", 0.0)).unwrap();
    let t = 0.6;
    let rows = cues.tick(t, &mut |select| s.text_at(t, select));
    s.schedule(rows);
    render(&mut s, 64, 64, t, 18);
    assert_eq!(
        s.compositor().text_of("#captions").as_deref(),
        Some(">> @mango: he goes for the flank>> Big Ludwig: what a read"),
    );
}
