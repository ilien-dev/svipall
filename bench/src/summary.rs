//! Offline aggregation of the saved comparison runs, using the same standalone Rust tool.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn med(values: impl Iterator<Item = f64>) -> f64 {
    let mut v: Vec<_> = values.collect();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return 0.0;
    }
    (v[(v.len() - 1) / 2] + v[v.len() / 2]) / 2.0
}

pub fn summarize(runs: &[Value]) -> anyhow::Result<Value> {
    let mut groups: BTreeMap<(String, String, u64), BTreeMap<String, Vec<Value>>> = BTreeMap::new();
    let mut samples = BTreeSet::new();
    for run in runs {
        let label = run["label"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing label"))?;
        let arm = label.split('-').next().unwrap_or(label);
        let round = label.rsplit('-').next().unwrap_or(label);
        let set = run["set"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing set"))?;
        anyhow::ensure!(run["ended_unix"].is_number(), "incomplete run: {label}");
        for cell in run["cells"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing cells"))?
        {
            let position = cell["position"].as_u64().unwrap_or(0);
            let target = cell["target"].as_str().unwrap_or_default();
            anyhow::ensure!(
                samples.insert((
                    arm.to_string(),
                    set.to_string(),
                    round.to_string(),
                    target.to_string(),
                    position
                )),
                "duplicate sample in {label}"
            );
            groups
                .entry((arm.into(), set.into(), position))
                .or_default()
                .entry(round.into())
                .or_default()
                .push(cell.clone());
        }
    }
    let mut rows = Vec::new();
    for ((arm, set, position), rounds) in groups {
        let counts: Vec<_> = rounds
            .values()
            .map(|c| c.iter().filter(|c| c["delivered"] == true).count())
            .collect();
        let cells: Vec<_> = rounds.values().flatten().collect();
        let mut latencies: Vec<_> = cells
            .iter()
            .map(|c| c["secs"].as_f64().unwrap_or(0.0))
            .collect();
        latencies.sort_by(f64::total_cmp);
        let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
        let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
        for cell in &cells {
            *outcomes
                .entry(cell["outcome"].as_str().unwrap_or("unknown").into())
                .or_default() += 1;
            *reasons
                .entry(
                    cell["response"]["blocked_reason"]
                        .as_str()
                        .unwrap_or("none")
                        .into(),
                )
                .or_default() += 1;
        }
        rows.push(json!({"arm":arm,"set":set,"position":position,"runs":rounds.len(),
            "targets":rounds.values().next().map_or(0,Vec::len),"per_run":counts,
            "median":med(counts.iter().map(|n| *n as f64)),"min":counts.iter().min(),"max":counts.iter().max(),
            "delivered":counts.iter().sum::<usize>(),"samples":cells.len(),
            "historical_ok_per_run":rounds.values().map(|c| c.iter().filter(|v| v["historical_verdict"] == "ok").count()).collect::<Vec<_>>(),
            "valid_public_ok_per_run":rounds.values().map(|c| c.iter().filter(|v| v["historical_verdict"] == "ok" && v["valid_status"] == true).count()).collect::<Vec<_>>(),
            "median_run_seconds":med(rounds.values().map(|c| c.iter().map(|v| v["secs"].as_f64().unwrap_or(0.0)).sum())),
            "median_page_seconds":med(latencies.iter().copied()),
            "p95_page_seconds":latencies.get((latencies.len()*95).div_ceil(100).saturating_sub(1)),
            "outcomes":outcomes,"reasons":reasons,
            "document_reused":cells.iter().filter(|c| c["response"]["warm"]["document_reused"] == true).count(),
            "renewals":cells.iter().filter(|c| c["response"]["warm"]["reissued"] == true).count()}));
    }
    Ok(json!({"summary":rows,"run_labels":runs.iter().map(|r| &r["label"]).collect::<Vec<_>>()}))
}

pub fn run(root: &Path) -> anyhow::Result<usize> {
    let mut runs = Vec::new();
    for entry in std::fs::read_dir(root.join("results"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|s| s == "json") {
            let text = std::fs::read_to_string(&path)?;
            let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
            if value["schema"] == 1 && value["cells"].is_array() {
                runs.push(value);
            }
        }
    }
    let result = summarize(&runs)?;
    std::fs::write(
        root.join("summary.json"),
        serde_json::to_string_pretty(&result)?,
    )?;
    let mut report = String::from("# Local before/after comparison\n\nFresh and returning visits are reported separately. Local state starts separately with identical identity seeds; server-side IP history is uncontrolled.\n\n| Arm | Set | Visit | Runs | Delivered median | Range | Median page seconds | p95 seconds |\n|---|---|---:|---:|---:|---|---:|---:|\n");
    for row in result["summary"].as_array().unwrap() {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {}..{} | {:.2} | {:.2} |\n",
            row["arm"].as_str().unwrap(),
            row["set"].as_str().unwrap(),
            row["position"],
            row["runs"],
            row["median"],
            row["targets"],
            row["min"],
            row["max"],
            row["median_page_seconds"].as_f64().unwrap_or(0.0),
            row["p95_page_seconds"].as_f64().unwrap_or(0.0)
        ));
    }
    report.push_str("\nThe original public verdict, valid-status scoring, errors, reuse and renewals remain separate in summary.json. Missing responses and timeouts stay in the denominator.\n");
    std::fs::write(root.join("comparison.md"), &report)?;
    println!("{report}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeouts_stay_in_denominator_and_repeats_are_separate() {
        let runs = vec![
            json!({"label":"before-public31-1","set":"public31","ended_unix":1,"cells":[
                {"target":"one","position":1,"secs":90,"outcome":"error","delivered":false,"valid_status":false,"historical_verdict":"ok","response":{"blocked_reason":"timeout"}},
                {"target":"one","position":2,"secs":1,"outcome":"delivered","delivered":true,"valid_status":true,"historical_verdict":"ok","response":{}}
            ]}),
        ];
        let s = summarize(&runs).unwrap();
        assert_eq!(s["summary"][0]["median"], 0.0);
        assert_eq!(s["summary"][0]["samples"], 1);
        assert_eq!(s["summary"][0]["historical_ok_per_run"][0], 1);
        assert_eq!(s["summary"][0]["valid_public_ok_per_run"][0], 0);
        assert_eq!(s["summary"][1]["median"], 1.0);
        assert!(summarize(&[runs[0].clone(), runs[0].clone()]).is_err());
    }
}
