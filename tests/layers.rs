//! The line between the application and the middleware, on both sides (D156, D211).
//!
//! Lecture 9 asks for the middleware and the application to be kept apart in the implementation as
//! well as the design, on the cloud side and on the client side. `src/middleware.rs` is the one
//! interface the application layer may use, and these tests fail when application code reaches past
//! it.
//!
//! They read the source, so they see only the code: comments go, strings are emptied, whitespace
//! goes, and each `#[cfg(test)] mod tests` goes, because a unit test may set up what it likes.
//! rustfmt breaks long calls across lines, so a plain search for `consensus.propose(` would pass on
//! code that breaks the rule (D156). `the_check_is_not_blind` holds the checker itself to account.
//!
//! What is not application is not checked. `client.rs` and `node/` may use each other, `bin/ctl.rs`
//! is an operator's tool, `bin/node.rs` starts a node, and `tests/` may reach anything.
//!
//! Owner: shared.

use std::path::PathBuf;

/// Application files on the cloud side (D156, D211).
const CLOUD_FILES: [&str; 5] = [
    "src/node/service.rs",
    "src/node/release.rs",
    "src/planner.rs",
    "src/campus.rs",
    "src/state.rs",
];
/// What in this crate they may name. Only `middleware` is from below; the rest is the application's
/// own, or the contract and the tools both layers share.
const CLOUD_MAY_NAME: [&str; 7] = [
    "campus",
    "config",
    "metrics",
    "middleware",
    "planner",
    "proto",
    "state",
];

/// What in the library the client side's application files may name: the users, as programs in
/// `bin/loadgen.rs` and as a web page in `bin/frontend.rs` (D211). `scripts/start-users.sh` is not
/// a file to check: it runs loadgen and reads its summary, and uses nothing of the library.
const CLIENT_MAY_NAME: [&str; 7] = [
    "campus",
    "config",
    "init_tracing",
    "metrics",
    "middleware",
    "proto",
    "shutdown_signal",
];

/// Names that only mean something below the line. An application file that says one has reached
/// past the interface, however it got there. A whole word and not `.consensus`, so that
/// `let NodeContext { consensus, .. } = ctx;` counts too.
const BELOW_THE_LINE: [&str; 18] = [
    // Raft.
    "consensus",
    "Consensus",
    "openraft",
    // The peers, and the sends the group is built on.
    ".peers",
    "PeerSet",
    "PeerBody",
    "PeerEnvelope",
    "broadcast",
    // What the detector and the fault gate keep to themselves.
    ".suspicion",
    ".gate",
    "FaultGate",
    // The wire, and the group send behind `submit`.
    "tarpc",
    "UdpSocket",
    "ClientRpc",
    "datagram_client",
    "connect_client",
    "group_send",
    // Only a child of `node/` or a test module could write this, and it gets round the interface.
    "super::",
];

/// Opening a socket. `bin/frontend.rs` is left out of this one: it listens for the browser on its
/// own PC and carries no cloud traffic (D200). `frontend_goes_through_the_interface_too` holds it
/// to that.
const SOCKETS: [&str; 4] = ["TcpStream", "TcpListener", "std::net", "tokio::net"];

// ---------------------------------------------------------------------------------------------
// Reading the source
// ---------------------------------------------------------------------------------------------

/// A file's code, as [`code_only`] gives it. The path comes at run time from what cargo sets, so
/// these tests can be run on a copy of the tree with a violation planted in it.
fn code_of(file: &str) -> String {
    let root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    let source = std::fs::read_to_string(root.join(file))
        .unwrap_or_else(|e| panic!("{file} must exist for this check to mean anything: {e}"));
    code_only(&source)
}

/// `source` with comments and test modules gone, strings and characters emptied, and no whitespace.
fn code_only(source: &str) -> String {
    let s: Vec<char> = source.chars().collect();
    let mut code = String::new();
    let mut i = 0;
    while i < s.len() {
        let next = s.get(i + 1).copied();
        match s[i] {
            '/' if next == Some('/') => {
                while i < s.len() && s[i] != '\n' {
                    i += 1;
                }
            }
            '/' if next == Some('*') => i = past_block_comment(&s, i),
            '"' => {
                i = past_string(&s, i);
                code.push_str("\"\"");
            }
            'r' | 'b' | 'c' => match raw_string_at(&s, i) {
                Some(end) => {
                    i = end;
                    code.push_str("\"\"");
                }
                None => {
                    code.push(s[i]);
                    i += 1;
                }
            },
            '\'' => match past_char_literal(&s, i) {
                Some(end) => {
                    i = end;
                    code.push_str("''");
                }
                // A lifetime.
                None => {
                    code.push('\'');
                    i += 1;
                }
            },
            c => {
                if !c.is_whitespace() {
                    code.push(c);
                }
                i += 1;
            }
        }
    }
    without_test_modules(code)
}

fn past_block_comment(s: &[char], start: usize) -> usize {
    let mut depth = 1;
    let mut i = start + 2;
    while i < s.len() && depth > 0 {
        match (s[i], s.get(i + 1).copied()) {
            ('/', Some('*')) => {
                depth += 1;
                i += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                i += 2;
            }
            _ => i += 1,
        }
    }
    i
}

fn past_string(s: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < s.len() && s[i] != '"' {
        i += if s[i] == '\\' { 2 } else { 1 };
    }
    i + 1
}

/// Where the raw string starting at `start` ends, if one does: `r"`, `r#"`, `br"`, `cr"`.
fn raw_string_at(s: &[char], start: usize) -> Option<usize> {
    let word_goes_on = start > 0 && (s[start - 1].is_alphanumeric() || s[start - 1] == '_');
    let mut i = start;
    if s[i] != 'r' {
        i += 1;
    }
    if word_goes_on || s.get(i) != Some(&'r') {
        return None;
    }
    i += 1;
    let hashes = s[i..].iter().take_while(|c| **c == '#').count();
    i += hashes;
    if s.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    while i < s.len() {
        if s[i] == '"'
            && s[i + 1..]
                .iter()
                .take(hashes)
                .filter(|c| **c == '#')
                .count()
                == hashes
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(i)
}

/// Where the character literal starting at `start` ends, or `None` if this `'` opens a lifetime.
fn past_char_literal(s: &[char], start: usize) -> Option<usize> {
    if s.get(start + 1) == Some(&'\\') {
        let close = s[start + 2..].iter().skip(1).position(|c| *c == '\'')?;
        return Some(start + 2 + 1 + close + 1);
    }
    (s.get(start + 2) == Some(&'\'')).then_some(start + 3)
}

/// Cuts `#[cfg(test)] mod tests { ... }`, and only that, so code after a test module still counts.
fn without_test_modules(mut code: String) -> String {
    const OPEN: &str = "#[cfg(test)]modtests{";
    while let Some(start) = code.find(OPEN) {
        let body = start + OPEN.len();
        let mut depth = 1;
        let mut end = code.len();
        for (at, c) in code[body..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                end = body + at + 1;
                break;
            }
        }
        code.replace_range(start..end, "");
    }
    code
}

// ---------------------------------------------------------------------------------------------
// The rule
// ---------------------------------------------------------------------------------------------

/// Every way `code` reaches past the interface: a word from `words`, or a path from `root` to
/// anything but `may_name`. Paths are checked against what is allowed and not against what is
/// forbidden, so `use rescuemesh::{client::MeshClient};` and a module added next year both count.
fn reach_past(code: &str, root: &str, may_name: &[&str], words: &[&[&str]]) -> Vec<String> {
    let mut found: Vec<String> = words
        .iter()
        .flat_map(|list| list.iter())
        .filter(|word| code.contains(**word))
        .map(|word| format!("names {word}"))
        .collect();
    for (at, _) in code.match_indices(root) {
        let rest = &code[at + root.len()..];
        match rest.strip_prefix("::") {
            Some(path) => {
                let name: String = path
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !may_name.contains(&name.as_str()) {
                    let head: String = path.chars().take(20).collect();
                    found.push(format!("names {root}::{head}"));
                }
            }
            // `pub(crate)` is a visibility. `use rescuemesh as rm;` is a way around this test.
            None if root != "crate" => found.push(format!("uses {root} without a path")),
            None => {}
        }
    }
    found
}

/// The cloud side's rule. Every file also keeps away from sockets.
fn cloud_reach_past(code: &str) -> Vec<String> {
    reach_past(code, "crate", &CLOUD_MAY_NAME, &[&BELOW_THE_LINE, &SOCKETS])
}

/// The client side's rule, without the socket words: see [`SOCKETS`].
fn client_reach_past(code: &str) -> Vec<String> {
    reach_past(code, "rescuemesh", &CLIENT_MAY_NAME, &[&BELOW_THE_LINE])
}

// ---------------------------------------------------------------------------------------------
// Cloud side
// ---------------------------------------------------------------------------------------------

#[test]
fn application_files_stay_above_the_group() {
    for file in CLOUD_FILES {
        let found = cloud_reach_past(&code_of(file));
        assert!(found.is_empty(), "{file} is application code: {found:#?}");
    }
}

/// A file that needs nothing from below passes the test above by doing nothing. These two have to
/// reach Raft and do, by the group's one call, so the interface is really what carries them.
#[test]
fn what_the_cloud_application_commits_goes_through_the_group() {
    for file in ["src/node/service.rs", "src/node/release.rs"] {
        let code = code_of(file);
        assert!(code.contains("crate::middleware::"), "{file}");
        assert!(code.contains(".group.send_ordered("), "{file}");
    }
}

// ---------------------------------------------------------------------------------------------
// Client side
// ---------------------------------------------------------------------------------------------

/// User code makes one call. It has no way to name the nodes or the copies behind that call.
#[test]
fn loadgen_imports_only_through_the_facade() {
    let code = code_of("src/bin/loadgen.rs");
    let found = client_reach_past(&code);
    assert!(found.is_empty(), "{found:#?}");
    let sockets = reach_past(&code, "rescuemesh", &CLIENT_MAY_NAME, &[&SOCKETS]);
    assert!(sockets.is_empty(), "{sockets:#?}");
    assert!(code.contains("rescuemesh::middleware::"));
    assert!(code.contains("client.submit("));
}

/// The page may hold sockets for its browser (D200), and only that: a connection it opened itself
/// could only be going past `MeshClient`, to a node.
#[test]
fn frontend_goes_through_the_interface_too() {
    let code = code_of("src/bin/frontend.rs");
    let found = client_reach_past(&code);
    assert!(found.is_empty(), "{found:#?}");
    assert!(
        !code.contains("TcpStream::connect"),
        "the page opened a connection of its own"
    );
    assert!(code.contains("rescuemesh::middleware::"));
    assert!(code.contains("client.submit("));
}

/// D126 and D210: this is the client middleware's send to all of the cloud nodes. It stays private,
/// so `submit` is the only way to it, and the interface does not hand it out.
#[test]
fn the_group_send_stays_behind_submit() {
    let code = code_of("src/client.rs");
    assert!(code.contains("fngroup_send("), "the group send has a name");
    assert!(code.contains("self.group_send("), "and submit uses it");
    assert!(!code.contains("pubfngroup_send("), "it is not public");
    assert!(!code.contains(")fngroup_send("), "not even pub(crate)");
    assert!(!code_of("src/middleware.rs").contains("group_send"));
}

// ---------------------------------------------------------------------------------------------
// The interface and the check
// ---------------------------------------------------------------------------------------------

/// Re-exports and nothing else, so what the two layers do stays in the two layers, and adding the
/// interface cannot have changed what the system does.
#[test]
fn the_interface_holds_no_code_of_its_own() {
    let code = code_of("src/middleware.rs");
    assert!(code.contains("pubusecrate::client::"));
    assert!(code.contains("pubusecrate::node::"));
    for statement in code.split(';').filter(|s| !s.is_empty()) {
        assert!(
            statement.starts_with("pubuse") || statement.starts_with("pub(crate)use"),
            "not a re-export: {statement}"
        );
    }
}

/// Each way out is one this checker has to see, and each clean line one it must not trip on.
#[test]
fn the_check_is_not_blind() {
    let cloud = |source: &str| cloud_reach_past(&code_only(source));
    let client = |source: &str| client_reach_past(&code_only(source));

    for way_out in [
        // As rustfmt writes it (D156): `consensus.propose(` is nowhere in this text.
        "let outcome = ctx\n    .consensus\n    .propose(cmd)\n    .await;",
        "let NodeContext { consensus, .. } = ctx;",
        "use crate::raft::store::StateReader;",
        "use crate::{raft};",
        "use crate::node::usage;",
        "use super::lock;",
        "ctx.peers.broadcast(1, deadline, body);",
        "let s = tokio::net::TcpStream::connect(addr);",
        // A test module ends, and the code after it still counts.
        "#[cfg(test)]\nmod tests { fn t() { let a = '{'; } }\nfn f(c: C) { c.consensus.status(); }",
    ] {
        assert!(
            !cloud(way_out).is_empty(),
            "missed on the cloud side: {way_out}"
        );
    }
    for way_out in [
        "use rescuemesh::client::MeshClient;",
        "use rescuemesh::{client::MeshClient};",
        "use rescuemesh::node::usage;",
        "use rescuemesh as rm;",
        "use rescuemesh::raft::RaftNode;",
        "use rescuemesh::new_module::Thing;",
        "client.group_send(deadline, request);",
        "let mut stub = tarpc::client::new(config, transport);",
    ] {
        assert!(
            !client(way_out).is_empty(),
            "missed on the client side: {way_out}"
        );
    }

    for clean in [
        "// ctx.consensus.propose(cmd) in a comment\nctx.group.send_ordered(cmd).await;",
        "/* use crate::raft; /* nested, .peers */ */ let n = ctx.group.term();",
        "/// crate::node::transport::PeerSet\nfn f() {}",
        "let url = \"http://x/.peers\"; // the next line must still be read\nlet y = 1;",
        "let raw = r#\"crate::raft \"consensus\" \"#;",
        "let quote = '\"'; let esc = '\\''; let t: &'static str = \"tarpc\";",
        "#[cfg(test)]\nmod tests { use crate::node::testing; fn t(c: C) { c.consensus.term(); } }",
        "pub(crate) fn f() {}",
    ] {
        assert_eq!(cloud(clean), Vec::<String>::new(), "tripped on: {clean}");
    }
    for clean in [
        "use rescuemesh::middleware::{MeshClient, control};\nrescuemesh::init_tracing();",
        "use rescuemesh::proto::Incident;",
        "#[command(about = \"Generate load against a rescuemesh cluster\")]\nstruct A;",
    ] {
        assert_eq!(client(clean), Vec::<String>::new(), "tripped on: {clean}");
    }

    // A string that holds `//` must not swallow the rest of its line, or a violation could hide
    // there.
    assert!(!cloud("let u = \"http://x\"; ctx.consensus.status();").is_empty());
}
