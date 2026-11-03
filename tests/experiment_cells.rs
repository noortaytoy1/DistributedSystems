//! Every experiment cell must be a valid config, and the profiles must hold the agreed cells (D63).
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::Path;

use rescuemesh::config::Config;

const BASE: &str = r#"
[[cluster.nodes]]
id = 1
peer_addr = "10.0.0.11:7001"
client_addr = "10.0.0.11:8001"

[metrics]
dir = "runs/current"

[storage]
dir = "data/raft"
"#;

fn cells() -> BTreeSet<String> {
    std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter_map(|name| name.strip_suffix(".toml").map(str::to_string))
        .collect()
}

fn cell_text(cell: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("experiments")
        .join(format!("{cell}.toml"));
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn every_cell_is_a_valid_config_with_a_run_line() {
    let cells = cells();
    assert_eq!(cells.len(), 32, "the full profile has 32 cells");
    for cell in &cells {
        let text = cell_text(cell);
        assert!(text.starts_with("# run:"), "{cell} has no run line");
        // The runner adds the nodes, metrics and storage, exactly as here.
        Config::from_toml(&format!("{BASE}{text}"))
            .unwrap_or_else(|e| panic!("{cell} is not a valid config: {e}"));
    }
}

#[test]
fn the_quick_profile_is_thirteen_cells_of_the_full_one() {
    let list = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments/quick.list"),
    )
    .unwrap();
    let quick: BTreeSet<String> = list.lines().map(str::to_string).collect();
    assert_eq!(quick.len(), 13);
    assert!(quick.is_subset(&cells()), "{quick:?}");
}
