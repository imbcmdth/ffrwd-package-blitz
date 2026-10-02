//! Parsing change rows, and the order they apply in.

use compose_core::changes::{Change, ClassOp, Row};
use compose_core::params::Params;

use crate::common::*;

#[test]
fn a_row_is_an_object_an_array_or_a_line_each() {
    let one = r##"{"at": 3.0, "select": "#stage", "change": "+lbar"}"##;
    let array = r##"[{"at": 3.0, "select": "#stage", "change": "+lbar"},
                     {"at": 3.0, "select": "#lower .name", "text": "Jane"}]"##;
    let ndjson = "{\"select\": \"a\", \"text\": \"x\"}\n\n{\"at\": 2, \"select\": \"b\", \"html\": \"<p>y</p>\"}";
    assert_eq!(Row::parse_list(one).unwrap().len(), 1);
    let rows = Row::parse_list(array).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].change,
        Change::Classes(vec![ClassOp::Add("lbar".into())])
    );
    assert_eq!(rows[1].change, Change::Text("Jane".into()));
    let rows = Row::parse_list(ndjson).unwrap();
    assert_eq!(rows[0].at, None);
    assert_eq!(rows[1].at, Some(2.0));
    assert_eq!(rows[1].change, Change::Html("<p>y</p>".into()));
    assert!(Row::parse_list("").unwrap().is_empty());
    // A list of rows whose elements are rows or arrays of rows, and lines
    // that are arrays.
    let nested = r##"[{"at": 2, "select": "#a", "text": "1"},
                      [{"at": 3, "select": "#b", "text": "2"}, {"at": 3, "select": "#c", "text": "3"}],
                      {"at": 4, "select": "#d", "text": "4"}]"##;
    let rows = Row::parse_list(nested).unwrap();
    let texts: Vec<_> = rows.iter().map(|r| r.select.as_str()).collect();
    assert_eq!(texts, ["#a", "#b", "#c", "#d"]);
    let lines = "[{\"select\": \"#a\", \"text\": \"1\"}, {\"select\": \"#b\", \"text\": \"2\"}]
{\"select\": \"#c\", \"text\": \"3\"}";
    assert_eq!(Row::parse_list(lines).unwrap().len(), 3);
    let (rows, errors) =
        Row::from_upstream(r##"[[{"select": "#a", "text": "1"}], {"select": "#b", "text": "2"}]"##);
    assert_eq!((rows.len(), errors.len()), (2, 0));
}

#[test]
fn rows_round_trip() {
    let src = r##"[{"at":1.5,"select":"div.foo","change":"-foo +bar ~baz"},
                  {"select":"#n","text":"hi"},
                  {"at":3,"select":"body","html":"<p>x</p>"}]"##;
    let rows = Row::parse_list(src).unwrap();
    let again: Vec<Row> = rows
        .iter()
        .map(|r| Row::from_value(&r.to_value()).unwrap())
        .collect();
    assert_eq!(rows, again);
}

#[test]
fn a_malformed_row_says_why() {
    for (row, why) in [
        (r#"{"at": 1}"#, "select"),
        (r#"{"select": "a"}"#, "exactly one"),
        (
            r#"{"select": "a", "text": "x", "html": "y"}"#,
            "exactly one",
        ),
        (r#"{"select": "a", "text": 5}"#, "exactly one"),
        (r#"{"at": "soon", "select": "a", "text": "x"}"#, "number"),
        (r#"{"select": "a", "change": "foo"}"#, "must start with"),
        (r#"[1, 2]"#, "objects"),
    ] {
        let err = Row::parse_list(row).unwrap_err();
        assert!(err.contains(why), "{row}: {err}");
    }
}

#[test]
fn upstream_rows_without_select_are_somebody_elses() {
    let (rows, errors) = Row::from_upstream(r#"{"text": "a caption", "start_t": 1}"#);
    assert!(rows.is_empty() && errors.is_empty());
    let (rows, errors) = Row::from_upstream(
        r##"[{"select": "#a", "text": "one"}, {"shot": 3}, {"select": "#b", "change": "+x"}]"##,
    );
    assert_eq!(rows.len(), 2);
    assert!(errors.is_empty());
    let (rows, errors) = Row::from_upstream(r##"{"select": "#a", "text": 1}"##);
    assert!(rows.is_empty());
    assert_eq!(errors.len(), 1);
    let (rows, errors) = Row::from_upstream("not json");
    assert!(rows.is_empty() && errors.is_empty());
}

#[test]
fn the_rows_parameter_takes_text_or_json() {
    let p = Params::parse(r##"{"rows": "[{\"select\":\"#a\",\"text\":\"x\"}]"}"##).unwrap();
    assert_eq!(p.rows.len(), 1);
    let p = Params::parse(r##"{"rows": [{"select":"#a","text":"x"},{"select":"#b","text":"y"}]}"##)
        .unwrap();
    assert_eq!(p.rows.len(), 2);
    let p = Params::parse(r##"{"rows": "{\"select\":\"#a\",\"text\":\"x\"}"}"##).unwrap();
    assert_eq!(p.rows.len(), 1);
    assert!(Params::parse(r##"{"rows": "[{\"select\":\"#a\"}]"}"##).is_err());
    assert!(Params::parse(r#"{"colour": "red"}"#).is_err());
    assert!(Params::parse(r#"{"fit": "cover"}"#).is_err());
    assert!(Params::parse(r#"{"css_width": 0}"#).is_err());
    let p = Params::parse(r#"{"css_width": "1280", "bypass": "false", "width": null}"#).unwrap();
    assert_eq!(p.css_width, Some(1280.0));
    assert!(!p.bypass);
    assert_eq!(p.width, None);
}

const ORDER_CSS: &str = "#a { position: absolute; left: 0; top: 0; width: 64px; height: 64px; background: #000; }\n#a.x { background: #fff; }";

fn class_of(html: &str, t_rows: &[(f64, &str)], at: f64) -> String {
    let mut s = session(html, 64, 64, 1);
    let mut n = 0;
    for (t, line) in t_rows {
        s.fold(*t, &lines(&[line]));
        n += 1;
    }
    let _ = n;
    render(&mut s, 64, 64, at, 0);
    let c = s.compositor();
    let doc = c.doc();
    let id = doc.get_element_by_id("a").unwrap();
    doc.get_node(id)
        .unwrap()
        .attr(blitz_dom::local_name!("class"))
        .unwrap_or("")
        .to_string()
}

#[test]
fn rows_at_one_time_apply_in_the_order_they_arrived() {
    let html = doc(ORDER_CSS, r#"<div id="a"></div>"#);
    // Within an array, array order.
    let add_then_remove =
        r##"[{"at":1,"select":"#a","change":"+x"},{"at":1,"select":"#a","change":"-x"}]"##;
    let remove_then_add =
        r##"[{"at":1,"select":"#a","change":"-x"},{"at":1,"select":"#a","change":"+x"}]"##;
    assert_eq!(class_of(&html, &[(0.0, add_then_remove)], 2.0), "");
    assert_eq!(class_of(&html, &[(0.0, remove_then_add)], 2.0), "x");
    // Across lines, the order received.
    assert_eq!(
        class_of(
            &html,
            &[
                (0.0, r##"{"at":1,"select":"#a","change":"+x"}"##),
                (0.0, r##"{"at":1,"select":"#a","change":"-x"}"##)
            ],
            2.0
        ),
        ""
    );
}

#[test]
fn rows_apply_in_at_order_whatever_order_they_arrive_in() {
    let html = doc(ORDER_CSS, r#"<div id="a">start</div>"#);
    let text = |at: f64| {
        let mut s = session(&html, 64, 64, 1);
        s.fold(
            0.0,
            &lines(&[
                r##"[{"at":2,"select":"#a","text":"two"},{"at":1,"select":"#a","text":"one"}]"##,
            ]),
        );
        render(&mut s, 64, 64, at, 0);
        let c = s.compositor();
        let doc = c.doc();
        let id = doc.get_element_by_id("a").unwrap();
        doc.get_node(id).unwrap().text_content()
    };
    assert_eq!(text(0.5), "start");
    assert_eq!(text(1.5), "one");
    assert_eq!(text(2.5), "two");
}
