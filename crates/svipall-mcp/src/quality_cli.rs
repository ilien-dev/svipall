//! The two halves of the substance classifier's life outside a fetch: getting labels, and turning
//! them into weights.
//!
//! Both are here rather than in a script because both are local. There is no framework to export
//! from and no service to call: the code that reads the training set and the code that reads the
//! model at run time are the same code, so they cannot drift apart.
//!
//! # On the labels
//!
//! DCLM's Figure 9 is the reason this is not simply "ask a big model what it thinks". The filter
//! that agreed *best* with human quality judgements performed *worst* as a filter, and agreement
//! with human labels explained under 30% of the variance in usefulness. Quality is relative to an
//! objective, and svipall's objective is "did this page let the caller answer the question" â so
//! the labels that matter are the ones drawn from what actually happened, not from what a page
//! looks like.
//!
//! `export-training` derives those from what this machine has already seen. It is honest about how
//! weak that is: the request log records what was fetched and when, not what the fetch was for, so
//! the signal is circumstantial and the export says so in every row it writes.

use serde::{Deserialize, Serialize};
use std::path::Path;
use svipall_core::cache::Store;
use svipall_core::quality::substance::{Config, Label, Model};

/// One labelled page, as it travels between the two commands.
///
/// The same shape whichever way it was labelled â from the log, from the dashboard, or by a large
/// model scoring a batch offline â so the trainer neither knows nor cares which.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub url: String,
    pub text: String,
    pub label: Label,
    /// Where the label came from, so a mixed set can be filtered or weighed by hand.
    pub source: String,
    /// Why, in one phrase. Written for a person reading the file, not for the trainer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// A fetch this quiet afterwards plausibly ended a line of enquiry.
///
/// Circumstantial and known to be: the log has no notion of a task, so "nothing followed" is the
/// only trace an answered question leaves. It is the signal DCLM says matters â what happened, not
/// what the page looked like â read through the only window this machine has on it.
const QUIET_SECS: i64 = 120;

/// Requests to one domain inside a `QUIET_SECS` window, above which the run is a sweep rather
/// than a person reading.
///
/// Four. A reader who opens an article, its two references and a related page in two minutes is
/// still reading; a crawl does forty. The two are not adjacent on this scale, so the exact value
/// matters less than that there is one.
const BURST: usize = 4;

/// Below this the log is too short to derive anything from, and a handful of rows would produce a
/// model that is confidently wrong.
const MIN_EXAMPLES: usize = 50;

/// Build a training set from what this machine has already fetched.
pub fn export_training(
    store: &Store,
    db: Option<&svipall_solver::db::Db>,
    out: &Path,
    since_days: i64,
) -> anyhow::Result<serde_json::Value> {
    let since = since_days.max(1) * 86_400;
    // Enough to cover a busy month; the cap is the store's own.
    let mut log = store.recent_requests(None, since, 500);
    log.sort_by_key(|l| l.at);

    let mut examples: Vec<Example> = Vec::new();
    for (i, line) in log.iter().enumerate() {
        if line.blocked || !(200..300).contains(&line.status) {
            continue;
        }
        let Some(page) = store.get(&line.url) else {
            continue;
        };
        if page.markdown.trim().is_empty() {
            continue;
        }

        // What the fetch already knows about the page, which is a floor rather than a label: a
        // husk is not substantive whatever happened afterwards.
        let integrity: Option<svipall_core::quality::Integrity> = page
            .quality
            .as_deref()
            .and_then(|q| serde_json::from_str(q).ok());
        let husk = integrity.as_ref().is_some_and(|q| !q.is_full());

        // ▲ Per domain, not globally. The rule reads "nothing followed" as "this answered the
        // question", and against the *global* sequence that is false twice over: a crawl of one
        // site makes every page of it `Ordinary` because the next page is a second behind, and an
        // idle machine makes everything `Substantive` because nothing follows anything. What the
        // signal is actually about is whether the reader kept digging *at this site*.
        let quiet_after = log
            .iter()
            .skip(i + 1)
            .find(|next| next.domain == line.domain)
            .map(|next| next.at - line.at >= QUIET_SECS)
            .unwrap_or(true);

        // And a sweep is not a reader. A crawl fetches a domain in bursts, and no page in a burst
        // carries any evidence about whether a person got what they came for.
        //
        // ⚠ An approximation. The log records what was fetched and when, and nothing marks a row
        // as crawl-originated; a column saying so would make this exact instead of close. Until
        // then, a burst is recognised by its shape.
        let burst = log
            .iter()
            .filter(|l| l.domain == line.domain && (l.at - line.at).abs() < QUIET_SECS)
            .count();
        if burst > BURST {
            continue;
        }

        let (label, why) = match (husk, quiet_after) {
            (true, _) => (Label::Thin, "the fetch judged it a husk"),
            (false, true) => (
                Label::Substantive,
                "nothing was fetched for two minutes afterwards",
            ),
            (false, false) => (Label::Ordinary, "the run carried on straight afterwards"),
        };

        examples.push(Example {
            url: line.url.clone(),
            text: page.markdown,
            label,
            source: "log".into(),
            why: Some(why.into()),
        });
    }

    // A person's judgement outranks a guess drawn from timing, so ratings are added after and a
    // later row for the same page is the one the trainer sees.
    let from_people = db
        .map(|d| from_dashboard(d, since_days))
        .unwrap_or_default();
    let rated = from_people.len();
    examples.extend(from_people);

    let mut body = String::new();
    for e in &examples {
        body.push_str(&serde_json::to_string(e)?);
        body.push('\n');
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, body)?;

    // ▲ Say what `--since` can actually reach. The request log is kept for fourteen days and the
    // page text for seven, so a row older than the page it points at yields nothing — and an
    // export that quietly returned half of what was asked for read as a quiet fortnight.
    let reachable = (svipall_core::cache::RetentionPolicy::default().page_ttl_secs / 86_400).max(1);
    let asked = since_days.max(1);

    Ok(serde_json::json!({
        "out_file": out.display().to_string(),
        "examples": examples.len(),
        "rated_by_a_person": rated,
        "by_label": counts(&examples),
        "days_asked_for": asked,
        "days_reachable": reachable.min(asked),
        "note": format!(
            "Weak supervision: the request log records what was fetched and when, never what the \
             fetch was for, so \"nothing followed within {QUIET_SECS}s at the same domain\" \
             stands in for \"this answered the question\". Three things it cannot do. It reaches \
             back {reachable} days however many are asked for, because that is how long page text \
             is kept even though the log runs longer. It never produces `junk`: the log has no \
             trace of a page being worthless, only of one being unremarkable, so a model fitted on \
             this alone has three levels and not four and the panel is where the fourth comes \
             from. And a page fetched in a burst is skipped, because a sweep is not a reader. Read \
             the `why` on each row before training on it, and mix in rows labelled by a person at \
             the dashboard or by a large model offline - the trainer takes any of them, in this \
             same format."
        ),
    }))
}

/// Put pages in front of a person to be rated.
///
/// The dashboard, the job table and the corpus already do everything a labelling loop needs â
/// present something, take an answer, keep it with who gave it â so this borrows all of it rather
/// than building a second one. The pages come from the cache, newest first, and are shown as the
/// *text a caller would have received* rather than as the page in a browser: what is being judged
/// is what svipall hands over.
pub fn ask(
    store: &Store,
    db: &svipall_solver::db::Db,
    count: usize,
    since_days: i64,
) -> anyhow::Result<serde_json::Value> {
    let since = since_days.max(1) * 86_400;
    let mut asked = 0usize;
    let mut seen = std::collections::HashSet::new();
    for line in store.recent_requests(None, since, 500) {
        if asked >= count {
            break;
        }
        if line.blocked || !seen.insert(line.url.clone()) {
            continue;
        }
        let Some(page) = store.get(&line.url) else {
            continue;
        };
        if page.markdown.trim().is_empty() {
            continue;
        }
        let payload = serde_json::json!({
            "url": line.url,
            "title": page.title,
            // Enough to judge by; the panel truncates again for the screen.
            "text": page.markdown.chars().take(8_000).collect::<String>(),
        });
        let job = db.create_local_job(
            Some(&line.url),
            "rating",
            "rate",
            Some(&payload.to_string()),
        )?;
        // The panel only draws jobs that are pending or waiting on a person, so this call is what
        // makes the job visible rather than an optional flourish.
        db.set_needs_human(&job.task_id, "how much of this page is actually in it?")?;
        asked += 1;
    }
    Ok(serde_json::json!({
        "asked": asked,
        "note": "Rate them at the dashboard (web_status reports its URL), then run \
                 `svipall quality export-training` to collect the answers.",
    }))
}

/// The ratings a person has already given, as training examples.
fn from_dashboard(db: &svipall_solver::db::Db, since_days: i64) -> Vec<Example> {
    let since = chrono::Utc::now().timestamp() - since_days.max(1) * 86_400;
    db.corpus(since, Some("rate"), Some("human"))
        .into_iter()
        .filter_map(|row| {
            let payload: serde_json::Value = serde_json::from_str(row.payload.as_deref()?).ok()?;
            let answer: serde_json::Value = serde_json::from_str(row.answer.as_deref()?).ok()?;
            let level: Label = serde_json::from_value(answer.get("level")?.clone()).ok()?;
            Some(Example {
                url: payload.get("url")?.as_str()?.to_string(),
                text: payload.get("text")?.as_str()?.to_string(),
                label: level,
                source: "human".into(),
                why: Some("a person read it and said so".into()),
            })
        })
        .collect()
}

fn counts(examples: &[Example]) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for l in Label::ALL {
        let n = examples.iter().filter(|e| e.label == *l).count();
        out.insert(l.as_str().into(), serde_json::json!(n));
    }
    serde_json::Value::Object(out)
}

/// Train a classifier from a JSONL file of examples and write it where svipall will find it.
pub fn train(
    input: &Path,
    out_dir: &Path,
    epochs: usize,
    lr: f32,
) -> anyhow::Result<serde_json::Value> {
    let body = std::fs::read_to_string(input)?;
    let mut examples: Vec<(String, Label)> = Vec::new();
    let mut skipped = 0usize;
    // ▲ One row per URL, last writer wins — and the rows are appended in the order the file was
    // built, which puts a person's judgement after the guess drawn from timing. Until this existed
    // the export's own claim that "a person's judgement outranks a guess drawn from timing" was
    // false: both rows went into the fit, and the weak label got an equal vote. Held-out accuracy
    // was worse than false, it was flattering — the same page appeared on both sides of the split.
    let mut by_url: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut superseded = 0usize;
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Example>(line) {
            Ok(e) if !e.text.trim().is_empty() => {
                let row = (e.text, e.label);
                match by_url.get(&e.url) {
                    Some(&i) if !e.url.trim().is_empty() => {
                        examples[i] = row;
                        superseded += 1;
                    }
                    _ => {
                        by_url.insert(e.url, examples.len());
                        examples.push(row);
                    }
                }
            }
            _ => skipped += 1,
        }
    }
    if examples.len() < MIN_EXAMPLES {
        anyhow::bail!(
            "{} usable examples; at least {MIN_EXAMPLES} are needed before a model is worth \
             fitting. Fewer than that produces one that is confidently wrong, which is worse than \
             having none â with no model the field is simply absent from a response.",
            examples.len()
        );
    }

    // Held out before a single pass, so the number reported at the end is not the number the model
    // was fitted on. Every fifth example, which keeps the label mix of the whole set.
    let (train, held): (Vec<_>, Vec<_>) = examples
        .iter()
        .enumerate()
        .partition(|(i, _)| !i.is_multiple_of(5));
    let train: Vec<(String, Label)> = train.into_iter().map(|(_, e)| e.clone()).collect();
    let held: Vec<(String, Label)> = held.into_iter().map(|(_, e)| e.clone()).collect();

    let mut model = Model::new(Config::default());
    for _ in 0..epochs.max(1) {
        model.fit(&train, lr);
    }

    let correct = held
        .iter()
        .filter(|(text, want)| model.predict(text).label == *want)
        .count();
    let accuracy = if held.is_empty() {
        0.0
    } else {
        correct as f64 / held.len() as f64
    };

    std::fs::create_dir_all(out_dir)?;
    let weights = out_dir.join("substance.bin");
    let sidecar = out_dir.join("substance.json");
    std::fs::write(&weights, model.to_bytes())?;
    std::fs::write(&sidecar, model.sidecar())?;

    Ok(serde_json::json!({
        "weights": weights.display().to_string(),
        "sidecar": sidecar.display().to_string(),
        "trained_on": train.len(),
        "held_out": held.len(),
        "skipped": skipped,
        // Rows a later row for the same URL replaced. A person.s judgement is appended after the
        // guess drawn from timing, so this is mostly the count of guesses that were overruled.
        "superseded": superseded,
        "held_out_accuracy": accuracy,
        "note": "Held-out accuracy is against the labels in the file, which says the model \
                 reproduces them â not that they were the right labels. What matters is whether \
                 the annotation helps a caller reach an answer in fewer fetches; that is measured \
                 against a question set, not here.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct URLs, because the trainer keeps one row per URL. A fixture of sixty rows all
    /// claiming to be the same page is one page, and the trainer is right to say so.
    fn example_at(url: &str, text: &str, label: Label) -> Example {
        Example {
            url: url.into(),
            text: text.into(),
            label,
            source: "test".into(),
            why: None,
        }
    }

    fn write_set(dir: &Path, n: usize) -> std::path::PathBuf {
        let path = dir.join("training.jsonl");
        let mut body = String::new();
        for i in 0..n {
            let a = example_at(
                &format!("https://x.test/a{i}"),
                &format!(
                    "the council voted on tuesday to approve the harbour measure after debate {i} \
                     supporters argued the change was overdue and the money was already set aside"
                ),
                Label::Substantive,
            );
            let b = example_at(
                &format!("https://x.test/b{i}"),
                &format!("best cheap anvils {i} buy now click here best price limited offer deal"),
                Label::Junk,
            );
            body.push_str(&serde_json::to_string(&a).unwrap());
            body.push('\n');
            body.push_str(&serde_json::to_string(&b).unwrap());
            body.push('\n');
        }
        std::fs::write(&path, body).unwrap();
        path
    }

    fn tempdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "svipall-quality-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_model_trained_from_a_file_is_written_where_svipall_looks_for_it() {
        let dir = tempdir();
        let set = write_set(&dir, 60);
        let out = dir.join("models");
        let report = train(&set, &out, 30, 0.5).expect("trained");

        assert!(out.join("substance.bin").is_file());
        assert!(out.join("substance.json").is_file());
        // And it round-trips through exactly the loader the server uses.
        let bytes = std::fs::read(out.join("substance.bin")).unwrap();
        let sidecar = std::fs::read_to_string(out.join("substance.json")).unwrap();
        let m = Model::load(&bytes, &sidecar).expect("loads");
        assert_eq!(
            m.predict("best cheap anvils buy now click here").label,
            Label::Junk
        );
        assert!(
            report["held_out_accuracy"].as_f64().unwrap() > 0.8,
            "{report}"
        );
    }

    #[test]
    fn too_few_examples_is_refused_rather_than_fitted() {
        // A model fitted on a handful is confidently wrong, and confidently wrong is worse than
        // absent: with no model at all the field simply does not appear.
        let dir = tempdir();
        let set = write_set(&dir, 4);
        let err = train(&set, &dir.join("models"), 5, 0.5).expect_err("must refuse");
        assert!(err.to_string().contains("at least"), "{err}");
    }

    #[test]
    fn the_score_it_reports_is_not_the_data_it_was_fitted_on() {
        let dir = tempdir();
        let set = write_set(&dir, 60);
        let report = train(&set, &dir.join("models"), 20, 0.5).expect("trained");
        let trained = report["trained_on"].as_u64().unwrap();
        let held = report["held_out"].as_u64().unwrap();
        assert!(held > 0, "nothing was held out: {report}");
        assert_eq!(trained + held, 120);
    }

    #[test]
    fn an_export_from_an_empty_history_produces_an_empty_set_and_says_so() {
        let dir = tempdir();
        let store = Store::open_memory().expect("store");
        let out = dir.join("training.jsonl");
        let report = export_training(&store, None, &out, 30).expect("exported");
        assert_eq!(report["examples"], 0);
        assert!(out.is_file(), "the file exists even when it is empty");
    }
}
