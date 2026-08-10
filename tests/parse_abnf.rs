use abnf::types::Node;
use abnf_to_pest::{parse_abnf, render_rules_to_pest, PestyRule};
use indexmap::IndexMap;

/// Whitespace-collapsed single-rule rendering.
fn rendered_single(rules: &IndexMap<String, PestyRule>, name: &str) -> String {
    let one = rules
        .get(name)
        .map(|r| PestyRule {
            silent: false,
            node: r.node.clone(),
        })
        .unwrap();
    let pretty = render_rules_to_pest(std::iter::once((name.to_string(), one)));
    pretty
        .pretty(80)
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn incremental_alternatives_collapse() {
    let abnf = "rule = A\nrule =/ B\nrule =/ C / D\n";
    let rules = parse_abnf(abnf).unwrap();

    let node = &rules.get("rule").unwrap().node;
    match node {
        Node::Alternatives(v) => {
            assert_eq!(v.len(), 4, "expected flat alternatives, got {:?}", node)
        }
        _ => panic!("expected an alternation, got {:?}", node),
    }
    assert_eq!(rendered_single(&rules, "rule"), "rule = { A | B | C | D }");
}

#[test]
fn incremental_without_base_is_initial() {
    // A stray `=/` with no preceding `=` is parsed as `Kind::Incremental`;
    // with nothing to augment, treat it as the initial definition.
    let abnf = "rule =/ A / B\n";
    let rules = parse_abnf(abnf).unwrap();
    assert!(rules.contains_key("rule"));
    assert_eq!(rendered_single(&rules, "rule"), "rule = { A | B }");
}

#[test]
fn empty_alternative_renders_last() {
    // pest implements ordered choice and pest_derive rejects an
    // alternative that cannot fail unless it comes last:
    //
    //     = expression cannot fail; following choices cannot be reached
    //
    // The empty alternative must render at the end, whether the
    // alternation was written in one rule or assembled with =/.
    let direct = parse_abnf("rule = \"\" / A / B / C\n").unwrap();
    let merged = parse_abnf("rule = \"\"\nrule =/ A / B\nrule =/ C\n").unwrap();
    assert_eq!(
        rendered_single(&direct, "rule"),
        "rule = { A | B | C | ^\"\" }"
    );
    assert_eq!(
        rendered_single(&merged, "rule"),
        "rule = { A | B | C | ^\"\" }"
    );
}

#[test]
fn longer_string_literals_render_first() {
    // Ordered choice also shadows later alternatives that merely extend an
    // earlier one: with { ^"foo" | ^"foobar" }, input "foobar" matches
    // "foo" and leaves "bar".  pest_derive accepts such a grammar without
    // complaint, so the emitted order alone decides whether the generated
    // parser is correct.  For plain string literals, longer-first is
    // PEG-safe, however the alternation was written.
    let direct = parse_abnf("rule = \"foo\" / \"foobar\"\n").unwrap();
    let merged = parse_abnf("rule = \"foo\"\nrule =/ \"foobar\"\n").unwrap();
    assert_eq!(
        rendered_single(&direct, "rule"),
        "rule = { ^\"foobar\" | ^\"foo\" }"
    );
    assert_eq!(
        rendered_single(&merged, "rule"),
        "rule = { ^\"foobar\" | ^\"foo\" }"
    );
}
