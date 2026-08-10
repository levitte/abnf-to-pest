#![doc(html_root_url = "https://docs.rs/abnf_to_pest/0.6.0")]

//! A tiny crate that helps convert ABNF grammars to [pest][pest].
//!
//! Example usage:
//! ```
//! let abnf_path = "src/grammar.abnf";
//! let pest_path = "src/grammar.pest";
//!
//! let mut file = File::open(abnf_path)?;
//! let mut data = Vec::new();
//! file.read_to_end(&mut data)?;
//! data.push('\n' as u8);
//!
//! let mut rules = abnf_to_pest::parse_abnf(&data)?;
//! rules.remove("some_inconvenient_rule");
//!
//! let mut file = File::create(pest_path)?;
//! writeln!(&mut file, "{}", render_rules_to_pest(rules).pretty(80))?;
//! ```
//!
//! [pest]: https://pest.rs

use abnf::types::{Kind, Node, Repeat, Rule, TerminalValues};
use indexmap::map::IndexMap;
use itertools::Itertools;
use pretty::BoxDoc;
use std::cmp::Ordering;
use std::collections::HashSet;

mod core_rules;
pub use core_rules::abnf_core_rules;

trait Pretty {
    fn pretty(&self) -> BoxDoc<'static>;
}

impl Pretty for Node {
    fn pretty(&self) -> BoxDoc<'static> {
        use Node::*;
        match self {
            Alternatives(nodes) => {
                // ABNF alternatives are an unordered set, while pest choice
                // is ordered, so render them in a PEG-safe order.
                let mut ordered: Vec<&Node> = nodes.iter().collect();
                ordered.sort_by(|a, b| choice_order(a, b));
                BoxDoc::intersperse(
                    ordered.iter().map(|x| x.pretty().nest(2).group()),
                    BoxDoc::space().append(BoxDoc::text("| ")),
                )
            }
            Concatenation(nodes) => BoxDoc::intersperse(
                nodes.iter().map(|x| x.pretty()),
                BoxDoc::space().append(BoxDoc::text("~ ")),
            ),
            Repetition { repeat, node } => node.pretty().append(repeat.pretty()),
            Rulename(s) => BoxDoc::text(escape_rulename(s)),
            Group(n) => BoxDoc::text("(")
                .append(n.pretty().nest(4).group())
                .append(BoxDoc::text(")")),
            Optional(n) => BoxDoc::text("(")
                .append(n.pretty().nest(4).group())
                .append(BoxDoc::text(")?")),
            String(s) => BoxDoc::text(format!(
                "{}\"{}\"",
                if s.is_case_sensitive() { "" } else { "^" },
                s.as_str().replace("\"", "\\\"").replace("\\", "\\\\")
            )),
            TerminalValues(r) => r.pretty(),
            Prose(_) => unimplemented!(),
        }
    }
}

/// Whether the pest expression rendered from `node` can succeed without
/// consuming any input.  In an ordered choice, such an expression shadows
/// every alternative that follows it, and pest_derive rejects it anywhere
/// but last.
fn cannot_fail(node: &Node) -> bool {
    use Node::*;
    match node {
        String(s) => s.as_str().is_empty(),
        Optional(_) => true,
        Repetition { repeat, .. } => repeat.min().unwrap_or(0) == 0,
        Group(n) => cannot_fail(n),
        Concatenation(v) => v.iter().all(cannot_fail),
        Alternatives(v) => v.iter().any(cannot_fail),
        Rulename(_) | TerminalValues(_) | Prose(_) => false,
    }
}

/// Order alternatives for pest's ordered choice: can't-fail expressions
/// last, and string literals longest first (a literal would otherwise
/// shadow any alternative it is a prefix of).  Anything else keeps its
/// written order.
fn choice_order(a: &Node, b: &Node) -> Ordering {
    match (cannot_fail(a), cannot_fail(b)) {
        (false, true) => return Ordering::Less,
        (true, false) => return Ordering::Greater,
        _ => {}
    }
    match (a, b) {
        (Node::String(x), Node::String(y)) => y.as_str().len().cmp(&x.as_str().len()),
        _ => Ordering::Equal,
    }
}

impl Pretty for Repeat {
    fn pretty(&self) -> BoxDoc<'static> {
        BoxDoc::text(match (self.min().unwrap_or(0), self.max()) {
            (0, None) => "*".into(),
            (1, None) => "+".into(),
            (0, Some(1)) => "?".into(),
            (min, None) => format!("{{{},}}", min),
            (min, Some(max)) if min == max => format!("{{{}}}", min),
            (min, Some(max)) => format!("{{{},{}}}", min, max),
        })
    }
}

impl Pretty for TerminalValues {
    fn pretty(&self) -> BoxDoc<'static> {
        use TerminalValues::*;
        BoxDoc::text(match self {
            Range(x, y) => {
                format!("'{}'..'{}'", format_char(*x), format_char(*y))
            }
            Concatenation(v) => {
                format!("\"{}\"", v.iter().map(|x| format_char(*x)).join(""))
            }
        })
    }
}

/// Escape the rule name to be a valid Rust identifier.
///
/// Replaces e.g. `if` with `if_`, and `rule-name` with `rule_name`.
/// Also changes `whitespace` to `whitespace_` because of https://github.com/pest-parser/pest/pull/374
/// Also changes `Some` and `None` to `Some_` and `None_`, because it was such a pain to work around.
pub fn escape_rulename(x: &str) -> String {
    let x = x.replace("-", "_");
    if x == "if"
        || x == "else"
        || x == "as"
        || x == "let"
        || x == "in"
        || x == "fn"
        // Not required but such a pain
        || x == "Some"
        || x == "None"
        // TODO: remove when https://github.com/pest-parser/pest/pull/375 gets into a release
        || x == "whitespace"
    {
        x + "_"
    } else {
        x
    }
}

fn format_char(x: u32) -> String {
    if x <= u32::from(u8::max_value()) {
        let x: u8 = x as u8;
        if x.is_ascii_graphic() {
            let x: char = x as char;
            if x != '"' && x != '\'' && x != '\\' {
                return x.to_string();
            }
        }
    }
    format!("\\u{{{:02X}}}", x)
}

/// Allow control over some of the pest properties of the outputted rule
#[derive(Debug, Clone)]
pub struct PestyRule {
    pub silent: bool,
    pub node: Node,
}

impl Pretty for (String, PestyRule) {
    fn pretty(&self) -> BoxDoc<'static> {
        let (name, rule) = self;
        BoxDoc::nil()
            .append(BoxDoc::text(name.clone()))
            .append(BoxDoc::text(" = "))
            .append(BoxDoc::text(if rule.silent { "_" } else { "" }))
            .append(BoxDoc::text("{"))
            .append(BoxDoc::space().append(rule.node.pretty()).nest(2))
            .append(BoxDoc::space())
            .append(BoxDoc::text("}"))
            .group()
    }
}

/// Parse an abnf file. Returns a map of rules.
pub fn parse_abnf(data: &str) -> Result<IndexMap<String, PestyRule>, std::io::Error> {
    let make_err = |e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e));
    let rules: Vec<Rule> = abnf::rulelist(data).map_err(make_err)?;

    let mut map: IndexMap<String, PestyRule> = IndexMap::new();
    for rule in rules {
        let name = escape_rulename(rule.name());
        let node = rule.node().clone();
        let node = if rule.kind() == Kind::Incremental {
            match map.remove(&name) {
                Some(existing) => merge_alternatives(existing.node, node),
                // No previous definition to augment; use this as the initial one.
                None => node,
            }
        } else {
            node
        };
        map.insert(
            name,
            PestyRule {
                silent: false,
                node,
            },
        );
    }
    Ok(map)
}

/// Combine `existing` and `new` into a single alternation, flattening any
/// `Node::Alternatives` on either side so repeated `=/` chains stay flat.
fn merge_alternatives(existing: Node, new: Node) -> Node {
    let mut alts = flatten_alternatives(existing);
    alts.extend(flatten_alternatives(new));
    Node::Alternatives(alts)
}

fn flatten_alternatives(node: Node) -> Vec<Node> {
    match node {
        Node::Alternatives(v) => v,
        other => vec![other],
    }
}

pub fn render_rules_to_pest<I>(rules: I) -> BoxDoc<'static>
where
    I: IntoIterator<Item = (String, PestyRule)>,
{
    let rules: Vec<(String, PestyRule)> = rules.into_iter().collect();
    let mut referenced: HashSet<String> = HashSet::new();
    for (_, rule) in &rules {
        collect_rulenames(&rule.node, &mut referenced);
    }
    let defined: HashSet<String> = rules.iter().map(|(name, _)| name.clone()).collect();
    let core_extras = core_rules::referenced_core_rules(&referenced, &defined);

    let pretty_rules = rules
        .into_iter()
        .chain(core_extras.into_iter())
        .map(|x| x.pretty());
    BoxDoc::intersperse(pretty_rules, BoxDoc::hardline())
}

/// Collect every rulename referenced within `node`, escaped as a pest rule
/// identifier. Needed to detect which core rules a grammar actually uses.
pub(crate) fn collect_rulenames(node: &Node, out: &mut HashSet<String>) {
    use Node::*;
    match node {
        Rulename(s) => {
            out.insert(escape_rulename(s));
        }
        Alternatives(v) | Concatenation(v) => {
            for n in v {
                collect_rulenames(n, out);
            }
        }
        Repetition { node, .. } => collect_rulenames(node, out),
        Group(n) | Optional(n) => collect_rulenames(n, out),
        String(_) | TerminalValues(_) | Prose(_) => {}
    }
}
