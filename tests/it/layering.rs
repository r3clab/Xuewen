use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const LAYERS: &[&[&str]] = &[
    &["text", "hash", "http", "pdf", "tasks", "config"],
    &["db"],
    &["library"],
    &[
        "metadata",
        "agent",
        "search",
        "daily",
        "summary",
        "translate",
        "preview",
        "annotations",
        "citations",
        "llm",
    ],
    &["ingest"],
    &["app"],
    &["web"],
    &["cli"],
];
const PEERS: usize = 3;
const CRATE_ROOT: &[&str] = &["lib.rs", "main.rs"];
const TEST_ONLY: &[&str] = &["testutil"];

fn rank(module: &str) -> Option<usize> {
    LAYERS.iter().position(|layer| layer.contains(&module))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

fn group_heads(group: &str) -> Vec<String> {
    let mut heads = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in group.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => {
                heads.push(ident(group[start..i].trim()));
                break;
            }
            '}' => depth -= 1,
            ',' if depth == 0 => {
                heads.push(ident(group[start..i].trim()));
                start = i + 1;
            }
            _ => {}
        }
    }
    heads.retain(|h| !h.is_empty() && h != "self");
    heads
}

fn crate_refs(source: &str) -> Vec<String> {
    let code: Vec<&str> = source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect();
    let code = code.join("\n");
    let mut refs = Vec::new();
    let mut rest = code.as_str();
    while let Some(i) = rest.find("crate::") {
        let before = rest[..i].chars().next_back();
        rest = &rest[i + "crate::".len()..];
        if before.is_some_and(|c| c == '$' || c == '_' || c.is_alphanumeric()) {
            continue;
        }
        match rest.strip_prefix('{') {
            Some(group) => refs.extend(group_heads(group)),
            None => refs.push(ident(rest)),
        }
    }
    refs
}

fn has_cycle(edges: &BTreeSet<(&str, &str)>) -> Option<Vec<String>> {
    fn visit<'a>(
        node: &'a str,
        edges: &BTreeSet<(&'a str, &'a str)>,
        path: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Option<Vec<String>> {
        if let Some(i) = path.iter().position(|n| *n == node) {
            return Some(path[i..].iter().map(|n| n.to_string()).collect());
        }
        if !done.insert(node) {
            return None;
        }
        path.push(node);
        for (_, to) in edges.iter().filter(|(from, _)| *from == node) {
            if let Some(cycle) = visit(to, edges, path, done) {
                return Some(cycle);
            }
        }
        path.pop();
        None
    }
    let mut done = BTreeSet::new();
    edges
        .iter()
        .find_map(|(from, _)| visit(from, edges, &mut Vec::new(), &mut done))
}

#[test]
fn src_modules_depend_only_on_lower_layers() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();

    let mut refs: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut layers_seen = BTreeSet::new();
    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&src).unwrap();
        if CRATE_ROOT.iter().any(|root| rel == Path::new(root)) {
            continue;
        }
        let first = rel.components().next().unwrap().as_os_str();
        let from = first.to_str().unwrap().trim_end_matches(".rs").to_string();
        if TEST_ONLY.contains(&from.as_str()) {
            continue;
        }
        let Some(from_rank) = rank(&from) else {
            violations.push(format!("src/{}: `{from}` has no layer", rel.display()));
            continue;
        };
        layers_seen.insert(from_rank);
        for to in crate_refs(&std::fs::read_to_string(file).unwrap()) {
            if to == from || TEST_ONLY.contains(&to.as_str()) {
                continue;
            }
            match rank(&to) {
                Some(to_rank)
                    if to_rank < from_rank || (from_rank == PEERS && to_rank == PEERS) => {}
                _ => violations.push(format!(
                    "src/{}: `{from}` uses `crate::{to}`",
                    rel.display()
                )),
            }
            *refs.entry((from.clone(), to)).or_default() += 1;
        }
    }
    assert!(
        violations.is_empty(),
        "layering violations:\n{}",
        violations.join("\n")
    );

    let peer_edges: BTreeSet<(&str, &str)> = refs
        .keys()
        .filter(|(from, to)| rank(from) == Some(PEERS) && rank(to) == Some(PEERS))
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();
    assert_eq!(has_cycle(&peer_edges), None, "feature modules form a cycle");

    assert_eq!(
        layers_seen.len(),
        LAYERS.len(),
        "a layer has no source files"
    );
    let total: usize = refs.values().sum();
    assert!(total > 200, "only {total} cross-module references found");
    assert!(
        refs.len() > 40,
        "only {} distinct module edges found",
        refs.len()
    );
    for (from, to) in [
        ("cli", "web"),
        ("web", "app"),
        ("app", "ingest"),
        ("ingest", "metadata"),
        ("metadata", "library"),
        ("library", "db"),
        ("library", "hash"),
        ("daily", "search"),
    ] {
        assert!(
            refs.contains_key(&(from.to_string(), to.to_string())),
            "no `{from}` -> `{to}` reference found"
        );
    }
}
