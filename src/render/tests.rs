use super::via_suffix;
use crate::json;
use crate::model::{CrateResult, CrateStatus, DepDiff, Refs, Report};
use crate::style::Style;

#[test]
fn via_suffix_matches_lock_output() {
    let style = Style {
        dim: "<d>",
        reset: "</>",
        ..Style::default()
    };
    assert_eq!(
        via_suffix(&style, Some("direct-a,direct-b")),
        " <d>via direct-a,direct-b</>"
    );
    assert_eq!(via_suffix(&style, Some("")), "");
    assert_eq!(via_suffix(&style, None), "");
}

#[test]
fn json_shape_and_field_order_match_the_documented_schema() {
    let report = Report {
        refs: Refs {
            baseline: "base-ref".to_string(),
            baseline_label: "merge-base(main, HEAD)".to_string(),
            baseline_sha: "1111111111111111111111111111111111111111".to_string(),
            baseline_short: "1111111".to_string(),
            head_ref: "HEAD".to_string(),
            head_label: "working tree".to_string(),
            head_sha: "2222222222222222222222222222222222222222".to_string(),
            head_short: "2222222".to_string(),
            head_is_worktree_snapshot: true,
        },
        deps: DepDiff::default(),
        crates: vec![CrateResult {
            name: "sample".to_string(),
            removed: 0,
            changed: 0,
            added: 0,
            removed_lines: Vec::new(),
            changed_lines: Vec::new(),
            added_lines: Vec::new(),
            status: CrateStatus::Ok,
            error: None,
            pubdep: Vec::new(),
        }],
        crate_count: 1,
        values: Vec::new(),
        docs: Vec::new(),
        removed_total: 0,
        changed_total: 0,
        added_total: 0,
        changed_crate_count: 0,
        error_crate_count: 0,
        pubdep_break_total: 0,
        pubdep_review_total: 0,
    };
    assert_eq!(
        json::emit(&report),
        r#"{
  "baseline": "merge-base(main, HEAD)",
  "baseline_sha": "1111111",
  "head": "working tree",
  "head_sha": "2222222",
  "verdict": "ok",
  "totals": {
    "removed": 0,
    "changed": 0,
    "added": 0,
    "api_breaking": 0,
    "dep_breaking": 0,
    "error_crates": 0,
    "value_changed": 0,
    "doc_changed": 0,
    "public_dep_breaking": 0
  },
  "deps": {
    "removed": [],
    "changed": [],
    "added": []
  },
  "values": [],
  "docs": [],
  "public_dep_breaks": [],
  "crates": [
    {
      "name": "sample",
      "removed": 0,
      "changed": 0,
      "added": 0,
      "status": "ok",
      "error": null
    }
  ]
}"#
    );
}
