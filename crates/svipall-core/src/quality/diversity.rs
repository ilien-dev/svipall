//! Ordering a set of results so the different ones are near the top.
//!
//! ▲ This is the ordering half of the anti-discard contract, and it is a *reordering only*. Nothing
//! is dropped, nothing is truncated, every result the caller asked for comes back. What changes is
//! which one they read first.
//!
//! Cuconasu et al. (SIGIR 2024) measured what actually degrades a generated answer, and it is not
//! the obviously irrelevant document: it is the high-scoring, on-topic, **answer-free** one, which
//! is exactly what five near-identical copies of the same wire story are. Their other finding is
//! the one this implements — adding *distant* documents, ones far from the query in embedding
//! space, **raised** accuracy. So the far result is the one worth putting where it will be read.
//!
//! Maximal Marginal Relevance (Carbonell & Goldstein, SIGIR 1998) is the standard shape:
//! repeatedly take whichever candidate maximises `λ·relevance − (1−λ)·max similarity to what is
//! already chosen`. Two adaptations to what svipall actually has:
//!
//! - **Relevance is the caller's own order.** There is no query-document score here — the caller
//!   passed a list of URLs and their order is their judgement. So relevance falls linearly with
//!   rank, and this can only ever move a result *against* that judgement when it is redundant.
//! - **Similarity is the simhash already computed.** Hamming distance over the 64-bit fingerprint
//!   every result already carries, so the ordering costs no extra pass over any text.
//!
//! The leader never moves. Whatever the caller put first is what they most wanted, and a diversity
//! rule that reorders the top result is answering a question nobody asked.

/// How much the caller's own ordering is worth against novelty.
///
/// ▲ Not a taste setting — it is the value that makes the module do its job, and the arithmetic
/// says where it has to be. For an exact copy sitting at rank 1 to lose to an entirely novel
/// result at the very bottom of a list of `n`:
///
/// ```text
/// λ·(1 − 1/n) − (1 − λ)·1  <  λ·(1/n)      ⟹  λ < n / (2(n − 1))
/// ```
///
/// which is 0.625 at five results, 0.526 at twenty, and tends to 0.5 as the list grows. At
/// Carbonell and Goldstein's "favour relevance" setting of 0.7 the inequality fails at every size
/// and four copies of one wire story stay stacked at the top — measured, and the reason this
/// constant is 0.5 and not 0.7.
///
/// It stays at the balanced value rather than going lower because a *merely similar* page must not
/// be able to leapfrog the caller's judgement: at 0.5 it takes near-identity to move a rank, and
/// results that are all distinct come back in exactly the order they were asked for.
pub const LAMBDA: f32 = 0.5;

/// How much of one document is already in the other, `0.0..=1.0`.
///
/// ▲ Not raw simhash similarity, and the difference is what makes this work at all. Two *unrelated*
/// documents do not score 0: each bit of a simhash is an independent coin flip between them, so
/// they differ in about 32 of 64 bits and score **0.5**. Feeding that straight into MMR charges
/// every candidate a large constant penalty, which cancels out of the comparison and leaves the
/// caller's order untouched — measured, on three copies of a wire story and one unrelated page,
/// where the unrelated page still finished last.
///
/// So the scale is rebased on what "unrelated" actually looks like: 0.5 similarity and below is
/// zero redundancy, 1.0 is total. Everything the penalty charges for is now agreement beyond
/// chance.
fn redundancy(a: u64, b: u64) -> f32 {
    let similarity = 1.0 - crate::dedup::hamming(a, b) as f32 / 64.0;
    (2.0 * similarity - 1.0).max(0.0)
}

/// The order to present these results in, as indices into the input.
///
/// A permutation, always: `order(h).len() == h.len()` and every index appears once. That is the
/// contract, and the test that says so is the one standing between this and a filter.
pub fn order(hashes: &[u64]) -> Vec<usize> {
    if hashes.len() < 3 {
        // Two results cannot be reordered into anything more diverse than they already are, and
        // the first one never moves.
        return (0..hashes.len()).collect();
    }
    let n = hashes.len();
    let relevance = |i: usize| 1.0 - i as f32 / n as f32;

    let mut chosen: Vec<usize> = vec![0];
    let mut left: Vec<usize> = (1..n).collect();
    while !left.is_empty() {
        let mut best = 0usize;
        let mut best_score = f32::NEG_INFINITY;
        for (slot, &i) in left.iter().enumerate() {
            let worst = chosen
                .iter()
                .map(|&j| redundancy(hashes[i], hashes[j]))
                .fold(0.0f32, f32::max);
            let score = LAMBDA * relevance(i) - (1.0 - LAMBDA) * worst;
            // Strictly greater, so an exact tie keeps the caller's order. Every comparison in this
            // loop is between candidates the caller already ranked.
            if score > best_score {
                best_score = score;
                best = slot;
            }
        }
        chosen.push(left.remove(best));
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ▲ The property that makes this an ordering and not a filter. If this test ever fails,
    /// something is being thrown away.
    #[test]
    fn every_result_comes_back_exactly_once() {
        for n in 0..12usize {
            let hashes: Vec<u64> = (0..n).map(|i| (i as u64) * 0x9e37_79b9).collect();
            let mut got = order(&hashes);
            assert_eq!(got.len(), n, "{n} results in, {} out", got.len());
            got.sort_unstable();
            assert_eq!(
                got,
                (0..n).collect::<Vec<_>>(),
                "not a permutation at n={n}"
            );
        }
    }

    #[test]
    fn the_callers_first_choice_stays_first() {
        let hashes = vec![0u64, 0, 0, 0, u64::MAX];
        assert_eq!(order(&hashes).first(), Some(&0));
    }

    /// Four copies of one story and one different page. The different one is what the caller
    /// should read second — it is the only result that can add anything.
    #[test]
    fn the_one_different_result_is_promoted_past_the_copies() {
        let story = 0xdead_beef_dead_beefu64;
        let hashes = vec![story, story, story, story, !story];
        let got = order(&hashes);
        assert_eq!(got[0], 0);
        assert_eq!(
            got[1], 4,
            "the only page that says something else was left at the bottom: {got:?}"
        );
    }

    /// With nothing redundant to break, the caller's order is their judgement and stands.
    #[test]
    fn results_that_are_all_different_keep_the_order_they_arrived_in() {
        // Eight unrelated documents, which is what unrelated fingerprints look like: about half
        // the bits differing between any two, so the redundancy between them is near zero and
        // relevance is what separates them. SplitMix64, so the fixture is the same everywhere.
        //
        // Drawn from the stream rather than taken off the front of it: sixty-four fair bits have a
        // standard deviation of four, so a random pair lands inside half a dozen bits of the mean
        // often enough that one in a couple of dozen reads as faintly related. Those are excluded
        // here because this test is about the other case — the set with *nothing* redundant in it.
        let mut hashes: Vec<u64> = Vec::new();
        for i in 1..200u64 {
            let mut z = i.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            if hashes.iter().all(|&h| redundancy(h, z) == 0.0) {
                hashes.push(z);
            }
            if hashes.len() == 8 {
                break;
            }
        }
        assert_eq!(hashes.len(), 8, "the fixture could not be built");
        for (i, a) in hashes.iter().enumerate() {
            for b in &hashes[i + 1..] {
                assert_eq!(redundancy(*a, *b), 0.0, "the fixture is not unrelated");
            }
        }
        assert_eq!(order(&hashes), (0..8).collect::<Vec<_>>());
    }

    /// ▲ The rescale, pinned. Two unrelated documents agree on about half a simhash by chance, and
    /// charging for that half is what left the one useful result at the bottom of a real list.
    #[test]
    fn agreement_by_chance_costs_nothing_and_only_real_overlap_is_charged_for() {
        assert_eq!(
            redundancy(0xffff_ffff_ffff_ffff, 0xffff_ffff_ffff_ffff),
            1.0
        );
        // Half the bits differing is what two unrelated pages look like.
        assert_eq!(
            redundancy(0x0000_0000_ffff_ffff, 0xffff_ffff_ffff_ffff),
            0.0
        );
        assert_eq!(
            redundancy(0, u64::MAX),
            0.0,
            "and nothing below it goes negative"
        );
        // Three quarters agreeing is halfway up the scale.
        assert!((redundancy(0x0000_ffff_ffff_ffff, u64::MAX) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_short_list_is_left_alone() {
        assert_eq!(order(&[]), Vec::<usize>::new());
        assert_eq!(order(&[7]), vec![0]);
        assert_eq!(order(&[7, 7]), vec![0, 1]);
    }
}
