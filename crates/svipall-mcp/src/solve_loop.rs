//! The loop that decides what to try on a challenge, and in what order.
//!
//! What it replaces was a cascade written by hand: a flag per challenge kind, an `if` per widget,
//! and an order fixed at compile time by whoever added their case last. That shape costs a day per
//! widget and gets slower with every one added, which is the wrong direction when the target is
//! twenty-nine of them.
//!
//! Here a strategy is a value in a list. The loop asks the page what challenge it is showing *now*
//! — not what it showed when the tier picked it — and tries the strategies that handle that
//! modality, cheapest-and-most-likely first. Adding a widget adds no code here at all; adding a
//! *modality* adds one strategy.
//!
//! Everything is generic over the surface, so the whole loop is tested against a fake page with no
//! Chromium, no network, and no captcha. That is the point: the ordering, the budgets and the
//! handover are the parts that break, and they are the parts a real browser makes untestable.

use futures::future::BoxFuture;
use std::time::{Duration, Instant};
use svipall_core::widget::Modality;

/// What a strategy did with its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The challenge was answered. The loop verifies this against the page rather than trusting it.
    Solved,
    /// This strategy has nothing to say about what is on the page. Costs no attempt — the same
    /// doctrine the image labeller already follows: "I do not know" is an answer, not a failure.
    NotApplicable,
    /// It tried and was wrong. Costs an attempt, because the site counted it.
    Failed,
}

/// How the loop ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The page carried an answer before anything was tried.
    AlreadyAnswered,
    /// Cleared by this strategy.
    Solved { strategy: &'static str },
    /// The budget for the modality ran out. A person is the next step, not another guess.
    Exhausted { modality: Modality },
    /// Time ran out with no challenge ever recognised.
    TimedOut,
}

impl Outcome {
    pub fn cleared(&self) -> bool {
        matches!(self, Outcome::AlreadyAnswered | Outcome::Solved { .. })
    }
}

/// The page, reduced to what the loop needs from it.
///
/// Two questions and nothing else, so a test can answer them from a script.
pub trait Surface: Sync {
    /// Does the answer field already hold something the site will accept?
    fn settled(&self) -> BoxFuture<'_, bool>;
    /// What is being asked right now, if anything is.
    fn probe(&self) -> BoxFuture<'_, Option<Modality>>;
    /// Wait for the page to catch up after an attempt. A no-op in tests.
    fn settle(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// One way of answering one modality.
pub trait Strategy<S: Surface>: Sync {
    /// Stable across restarts: it is the key its success rate is recorded under.
    fn name(&self) -> &'static str;
    fn handles(&self, modality: Modality) -> bool;
    /// Roughly what it costs to try, in arbitrary units. A hash loop is 1; interrupting a person is
    /// not a strategy at all and never appears here.
    fn cost(&self) -> u32 {
        1
    }
    fn run<'a>(&'a self, surface: &'a S) -> BoxFuture<'a, anyhow::Result<Step>>;
}

/// What this machine has seen a strategy do before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Record {
    pub ok: u32,
    pub tried: u32,
    /// Mean time an attempt took, in milliseconds. Zero when nothing was measured.
    pub ms: u32,
}

/// Expected value per unit of cost.
///
/// Laplace smoothing is what stops a strategy that happened to work once from outranking one with a
/// hundred successes, and — just as important — stops a strategy that has never been tried from
/// being ranked last forever and therefore never tried. An untried strategy scores 1/2, which is
/// optimistic enough to get a turn and pessimistic enough not to jump the queue.
///
/// `cost` is the strategy's own estimate of what it costs to try; the measured latency, once
/// there is one, corrects it. A strategy that says it is cheap and takes eight seconds ranks like
/// one that takes eight seconds.
pub fn score(r: Record, cost: u32) -> f64 {
    let rate = (r.ok as f64 + 1.0) / (r.tried as f64 + 2.0);
    let seconds = r.ms as f64 / 1000.0;
    rate / cost.max(1) as f64 / (1.0 + seconds)
}

/// The strategies that handle `modality`, best first.
///
/// Ties break on name so the order is stable between runs; an order that changes on its own makes
/// every failure report unreproducible.
pub fn order<'a, S: Surface>(
    strategies: &[&'a dyn Strategy<S>],
    modality: Modality,
    history: &(dyn Fn(&str) -> Record + Sync),
) -> Vec<&'a dyn Strategy<S>> {
    let mut out: Vec<_> = strategies
        .iter()
        .copied()
        .filter(|s| s.handles(modality))
        .collect();
    out.sort_by(|a, b| {
        let (sa, sb) = (
            score(history(a.name()), a.cost()),
            score(history(b.name()), b.cost()),
        );
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name().cmp(b.name()))
    });
    out
}

/// Try to clear whatever the page is showing, within `wait`.
///
/// `report` is called once per attempt that actually cost something, with the modality it was
/// spent on and how long it took; that is what feeds the ordering on the next run. Attempts that
/// came back `NotApplicable` are not reported: a strategy that correctly declines is not a
/// strategy that failed, and recording it as one would bury it.
pub async fn run<S: Surface>(
    surface: &S,
    strategies: &[&dyn Strategy<S>],
    history: &(dyn Fn(&str) -> Record + Sync),
    report: &mut (dyn FnMut(&'static str, Modality, bool, Duration) + Send),
    wait: Duration,
) -> Outcome {
    // The cheapest possible answer: the page may have solved itself while the tier was deciding.
    if surface.settled().await {
        return Outcome::AlreadyAnswered;
    }
    let deadline = Instant::now() + wait;
    let mut spent = 0u8;
    let mut seen: Option<Modality> = None;
    loop {
        let Some(modality) = surface.probe().await else {
            if Instant::now() >= deadline {
                return match seen {
                    Some(m) => Outcome::Exhausted { modality: m },
                    None => Outcome::TimedOut,
                };
            }
            surface.settle().await;
            continue;
        };
        // A widget that swaps to a different challenge after a wrong answer starts a fresh budget:
        // the attempts already spent were spent on a different question.
        if seen != Some(modality) {
            seen = Some(modality);
            spent = 0;
        }
        if spent >= modality.attempt_budget() {
            return Outcome::Exhausted { modality };
        }
        let handlers = order(strategies, modality, history);
        if handlers.is_empty() {
            // Nothing here answers this modality at all. Spinning until the deadline only delays
            // telling the caller so.
            return Outcome::Exhausted { modality };
        }
        let mut progressed = false;
        for s in handlers {
            if Instant::now() >= deadline {
                return Outcome::Exhausted { modality };
            }
            let started = Instant::now();
            match s.run(surface).await {
                Ok(Step::NotApplicable) => continue,
                Ok(Step::Solved) => {
                    // The strategy says it worked; the page decides whether it did. Trusting the
                    // strategy here is how a wrong answer gets reported as a cleared wall.
                    surface.settle().await;
                    let cleared = surface.settled().await;
                    report(s.name(), modality, cleared, started.elapsed());
                    if cleared {
                        return Outcome::Solved { strategy: s.name() };
                    }
                    spent += 1;
                    progressed = true;
                    break;
                }
                Ok(Step::Failed) | Err(_) => {
                    report(s.name(), modality, false, started.elapsed());
                    spent += 1;
                    progressed = true;
                    break;
                }
            }
        }
        if !progressed {
            // Every strategy that handles this declined — usually because the widget has not
            // finished drawing: the words are on the page, the button is not. Measured on a hold
            // whose frame stays hidden for a while after the challenge is announced. Declining
            // costs no attempt, so the honest move is to give the page a moment and ask again,
            // not to report exhaustion and leave the caller to a person.
            if Instant::now() >= deadline {
                return Outcome::Exhausted { modality };
            }
            surface.settle().await;
            continue;
        }
        if Instant::now() >= deadline {
            return Outcome::Exhausted { modality };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    /// A page written as a script: what `probe` says on each call, and when it becomes settled.
    struct Fake {
        script: Mutex<Vec<Option<Modality>>>,
        settles_after: usize,
        probes: AtomicUsize,
        answers: AtomicUsize,
    }

    impl Fake {
        fn new(script: Vec<Option<Modality>>, settles_after: usize) -> Self {
            Self {
                script: Mutex::new(script),
                settles_after,
                probes: AtomicUsize::new(0),
                answers: AtomicUsize::new(0),
            }
        }
    }

    impl Surface for Fake {
        fn settled(&self) -> BoxFuture<'_, bool> {
            Box::pin(async move { self.answers.load(AtomicOrdering::SeqCst) >= self.settles_after })
        }
        fn probe(&self) -> BoxFuture<'_, Option<Modality>> {
            Box::pin(async move {
                self.probes.fetch_add(1, AtomicOrdering::SeqCst);
                let mut s = self.script.lock().expect("lock");
                if s.is_empty() {
                    None
                } else {
                    s.remove(0)
                }
            })
        }
    }

    struct Fixed {
        name: &'static str,
        modality: Modality,
        step: Step,
        cost: u32,
        calls: AtomicUsize,
    }

    impl Fixed {
        fn new(name: &'static str, modality: Modality, step: Step) -> Self {
            Self {
                name,
                modality,
                step,
                cost: 1,
                calls: AtomicUsize::new(0),
            }
        }
        fn costing(mut self, cost: u32) -> Self {
            self.cost = cost;
            self
        }
    }

    impl Strategy<Fake> for Fixed {
        fn name(&self) -> &'static str {
            self.name
        }
        fn handles(&self, m: Modality) -> bool {
            m == self.modality
        }
        fn cost(&self) -> u32 {
            self.cost
        }
        fn run<'a>(&'a self, surface: &'a Fake) -> BoxFuture<'a, anyhow::Result<Step>> {
            Box::pin(async move {
                self.calls.fetch_add(1, AtomicOrdering::SeqCst);
                if self.step == Step::Solved {
                    surface.answers.fetch_add(1, AtomicOrdering::SeqCst);
                }
                Ok(self.step.clone())
            })
        }
    }

    fn no_history(_: &str) -> Record {
        Record::default()
    }

    fn nothing(_: &'static str, _: Modality, _: bool, _: Duration) {}

    #[tokio::test]
    async fn a_page_that_already_carries_an_answer_costs_nothing_at_all() {
        // The tier may have cleared it while deciding. Trying anything here would risk replacing a
        // good token with a wrong one.
        let page = Fake::new(vec![Some(Modality::Token)], 0);
        let out = run(
            &page,
            &[],
            &no_history,
            &mut nothing,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(out, Outcome::AlreadyAnswered);
        assert_eq!(page.probes.load(AtomicOrdering::SeqCst), 0, "never probed");
    }

    #[tokio::test]
    async fn the_strategy_that_answers_is_the_one_reported() {
        let page = Fake::new(vec![Some(Modality::Nonce)], 1);
        let a = Fixed::new("wrong-modality", Modality::Tiles, Step::Solved);
        let b = Fixed::new("proof-of-work", Modality::Nonce, Step::Solved);
        let out = run(
            &page,
            &[&a, &b],
            &no_history,
            &mut nothing,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(
            out,
            Outcome::Solved {
                strategy: "proof-of-work"
            }
        );
        assert_eq!(
            a.calls.load(AtomicOrdering::SeqCst),
            0,
            "a strategy for another modality is never run"
        );
    }

    #[tokio::test]
    async fn declining_is_free_and_the_next_strategy_gets_its_turn() {
        // `NotApplicable` must not spend the budget, or three honest abstentions would exhaust a
        // modality that nothing had actually attempted.
        let page = Fake::new(vec![Some(Modality::Slide); 4], 1);
        let a = Fixed::new("abstains", Modality::Slide, Step::NotApplicable);
        let b = Fixed::new("answers", Modality::Slide, Step::Solved);
        let mut reported: Vec<(&'static str, bool)> = Vec::new();
        let out = run(
            &page,
            &[&a, &b],
            &no_history,
            &mut |n, _m, ok, _t| reported.push((n, ok)),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(
            out,
            Outcome::Solved {
                strategy: "answers"
            }
        );
        assert_eq!(a.calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            reported,
            vec![("answers", true)],
            "an abstention is not a failure and must not be recorded as one"
        );
    }

    #[tokio::test]
    async fn a_strategy_that_claims_success_is_checked_against_the_page() {
        // Believing the strategy is how a wrong answer gets reported as a cleared wall.
        let page = Fake::new(vec![Some(Modality::Text); 8], 99);
        let liar = Fixed::new("liar", Modality::Text, Step::Solved);
        let mut reported = Vec::new();
        let out = run(
            &page,
            &[&liar],
            &no_history,
            &mut |n, _m, ok, _t| reported.push((n, ok)),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(
            out,
            Outcome::Exhausted {
                modality: Modality::Text
            }
        );
        assert!(
            reported.iter().all(|(_, ok)| !ok),
            "the page never settled, so nothing succeeded: {reported:?}"
        );
    }

    #[tokio::test]
    async fn the_budget_is_the_modalitys_own_and_running_out_hands_over() {
        let budget = Modality::Tiles.attempt_budget();
        let page = Fake::new(vec![Some(Modality::Tiles); 20], 99);
        let s = Fixed::new("always-wrong", Modality::Tiles, Step::Failed);
        let out = run(
            &page,
            &[&s],
            &no_history,
            &mut nothing,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            out,
            Outcome::Exhausted {
                modality: Modality::Tiles
            }
        );
        assert_eq!(
            s.calls.load(AtomicOrdering::SeqCst) as u8,
            budget,
            "a site counts every wrong answer, so the budget is a hard stop"
        );
    }

    #[tokio::test]
    async fn a_challenge_that_changes_shape_gets_a_fresh_budget() {
        // Widgets escalate: a wrong slide becomes a tile grid. The attempts spent on the slider
        // were spent on a different question and must not count against the new one.
        let mut script = vec![Some(Modality::Slide); Modality::Slide.attempt_budget() as usize];
        script.push(Some(Modality::Nonce));
        let page = Fake::new(script, 1);
        let bad = Fixed::new("slide", Modality::Slide, Step::Failed);
        let good = Fixed::new("nonce", Modality::Nonce, Step::Solved);
        let out = run(
            &page,
            &[&bad, &good],
            &no_history,
            &mut nothing,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(out, Outcome::Solved { strategy: "nonce" });
    }

    #[tokio::test]
    async fn nothing_recognised_before_the_deadline_is_a_timeout_not_a_failure() {
        // The distinction matters: "no challenge found" means look somewhere else, "exhausted"
        // means fetch a person.
        let page = Fake::new(vec![None; 3], 99);
        let out = run(
            &page,
            &[],
            &no_history,
            &mut nothing,
            Duration::from_millis(50),
        )
        .await;
        assert_eq!(out, Outcome::TimedOut);
    }

    #[tokio::test]
    async fn a_modality_no_strategy_handles_is_reported_at_once() {
        // Spinning until the deadline only delays telling the caller that nobody can answer this.
        let page = Fake::new(vec![Some(Modality::Polygon); 5], 99);
        let s = Fixed::new("elsewhere", Modality::Audio, Step::Solved);
        let out = run(
            &page,
            &[&s],
            &no_history,
            &mut nothing,
            Duration::from_secs(30),
        )
        .await;
        assert_eq!(
            out,
            Outcome::Exhausted {
                modality: Modality::Polygon
            }
        );
    }

    /// Declines until the widget "has drawn", then answers.
    struct Later {
        ready_after: usize,
        calls: AtomicUsize,
    }

    impl Strategy<Fake> for Later {
        fn name(&self) -> &'static str {
            "later"
        }
        fn handles(&self, m: Modality) -> bool {
            m == Modality::Hold
        }
        fn run<'a>(&'a self, surface: &'a Fake) -> BoxFuture<'a, anyhow::Result<Step>> {
            Box::pin(async move {
                let n = self.calls.fetch_add(1, AtomicOrdering::SeqCst);
                if n < self.ready_after {
                    return Ok(Step::NotApplicable);
                }
                surface.answers.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(Step::Solved)
            })
        }
    }

    #[tokio::test]
    async fn a_strategy_that_declines_now_gets_another_turn_once_the_widget_has_drawn() {
        // Measured on a hold whose frame stays hidden for a while after the challenge is
        // announced: the words are on the page, the button is not. Reporting exhaustion at the
        // first decline handed every one of those to a person.
        let page = Fake::new(vec![Some(Modality::Hold); 10], 1);
        let s = Later {
            ready_after: 3,
            calls: AtomicUsize::new(0),
        };
        let out = run(
            &page,
            &[&s],
            &no_history,
            &mut nothing,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(out, Outcome::Solved { strategy: "later" });
        assert_eq!(
            s.calls.load(AtomicOrdering::SeqCst),
            4,
            "three declines, then the answer"
        );
    }

    #[test]
    fn what_worked_here_before_goes_first() {
        let a = Fixed::new("proven", Modality::Tiles, Step::Solved);
        let b = Fixed::new("unproven", Modality::Tiles, Step::Solved);
        let list: Vec<&dyn Strategy<Fake>> = vec![&b, &a];
        let history = |n: &str| match n {
            "proven" => Record {
                ok: 40,
                tried: 50,
                ms: 0,
            },
            _ => Record::default(),
        };
        let ordered = order(&list, Modality::Tiles, &history);
        assert_eq!(ordered[0].name(), "proven");
    }

    #[test]
    fn something_never_tried_still_gets_a_turn_ahead_of_something_that_never_works() {
        // Ranking untried strategies last is how a good strategy stays untried forever.
        let a = Fixed::new("always-fails", Modality::Tiles, Step::Failed);
        let b = Fixed::new("never-tried", Modality::Tiles, Step::Solved);
        let list: Vec<&dyn Strategy<Fake>> = vec![&a, &b];
        let history = |n: &str| match n {
            "always-fails" => Record {
                ok: 0,
                tried: 60,
                ms: 0,
            },
            _ => Record::default(),
        };
        let ordered = order(&list, Modality::Tiles, &history);
        assert_eq!(ordered[0].name(), "never-tried");
    }

    #[test]
    fn a_cheap_strategy_wins_a_tie_on_success_rate() {
        let a = Fixed::new("expensive", Modality::Text, Step::Solved).costing(10);
        let b = Fixed::new("cheap", Modality::Text, Step::Solved).costing(1);
        let list: Vec<&dyn Strategy<Fake>> = vec![&a, &b];
        let same = |_: &str| Record {
            ok: 9,
            tried: 10,
            ms: 0,
        };
        assert_eq!(order(&list, Modality::Text, &same)[0].name(), "cheap");
    }

    #[test]
    fn the_order_does_not_change_between_two_identical_runs() {
        // An order that shuffles on its own makes every failure report unreproducible.
        let a = Fixed::new("aaa", Modality::Text, Step::Solved);
        let b = Fixed::new("bbb", Modality::Text, Step::Solved);
        let list: Vec<&dyn Strategy<Fake>> = vec![&b, &a];
        let names = |l: &[&dyn Strategy<Fake>]| l.iter().map(|s| s.name()).collect::<Vec<_>>();
        assert_eq!(
            names(&order(&list, Modality::Text, &no_history)),
            names(&order(&list, Modality::Text, &no_history))
        );
    }

    #[test]
    fn an_untried_strategy_is_neither_certain_nor_hopeless() {
        assert!((score(Record::default(), 1) - 0.5).abs() < 1e-9);
        assert!(
            score(
                Record {
                    ok: 9,
                    tried: 10,
                    ms: 0
                },
                1
            ) > score(Record::default(), 1)
        );
        assert!(
            score(
                Record {
                    ok: 0,
                    tried: 10,
                    ms: 0
                },
                1
            ) < score(Record::default(), 1)
        );
    }
}
