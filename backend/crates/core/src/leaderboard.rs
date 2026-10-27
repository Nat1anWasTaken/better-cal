//! Round scoring: ICPC-style ranking (problems solved, then penalty) plus
//! badges. Times are seconds since the round started, so this stays free of
//! clocks and I/O.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// Penalty minutes for each wrong submit before the first accept.
pub const PENALTY_PER_WRONG: i64 = 20;

pub const BADGE_FIRST_SOLVE: &str = "first_solve";
pub const BADGE_NO_HINTS: &str = "no_hints";

/// One judged `submit` inside the round window.
#[derive(Debug, Clone)]
pub struct Submit {
    pub user: Uuid,
    pub problem: Uuid,
    pub verdict: String,
    pub at: i64,
}

/// A hint level revealed during the round.
#[derive(Debug, Clone)]
pub struct HintReveal {
    pub user: Uuid,
    pub problem: Uuid,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Cell {
    pub problem_id: Uuid,
    pub solved: bool,
    /// Minutes from the round start to the first accept.
    pub minutes: Option<i64>,
    /// Wrong submits (compile errors excluded) before the first accept, or
    /// in total when unsolved.
    pub wrong_tries: u32,
    /// Submits made after the freeze whose results are hidden.
    pub pending: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Badge {
    /// `first_solve` or `no_hints`; more kinds may be added.
    pub kind: String,
    pub problem_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Standing {
    pub user_id: Uuid,
    /// 1-based; rows tied on solved, penalty and last accept share a rank.
    pub rank: u32,
    pub solved: u32,
    /// Minutes: time to each accept plus `PENALTY_PER_WRONG` per wrong try.
    pub penalty: i64,
    pub cells: Vec<Cell>,
    pub badges: Vec<Badge>,
    #[serde(skip)]
    last_accept: i64,
}

fn counts_as_wrong(verdict: &str) -> bool {
    verdict != "accepted" && verdict != "compile_error"
}

/// Rank `users` (in their tie-break display order) on `problems`. Submits at
/// or after `cutoff` count only as pending. Users without any submit in the
/// round are left out.
pub fn rank(problems: &[Uuid], users: &[Uuid], submits: &[Submit], hints: &[HintReveal], cutoff: Option<i64>) -> Vec<Standing> {
    let eligible: HashSet<Uuid> = users.iter().copied().collect();
    let wanted: HashSet<Uuid> = problems.iter().copied().collect();
    let mut subs: Vec<&Submit> = submits.iter().filter(|s| eligible.contains(&s.user) && wanted.contains(&s.problem)).collect();
    subs.sort_by_key(|s| s.at);

    // (user, problem) -> cell; walk submits in time order.
    let mut cells: HashMap<(Uuid, Uuid), Cell> = HashMap::new();
    let mut accepted_at: HashMap<(Uuid, Uuid), i64> = HashMap::new();
    let mut participants: HashSet<Uuid> = HashSet::new();
    for s in subs {
        participants.insert(s.user);
        let key = (s.user, s.problem);
        let cell = cells.entry(key).or_insert(Cell { problem_id: s.problem, solved: false, minutes: None, wrong_tries: 0, pending: 0 });
        if cutoff.is_some_and(|c| s.at >= c) {
            cell.pending += 1;
        } else if cell.solved {
            // Later submits after an accept don't change anything.
        } else if s.verdict == "accepted" {
            cell.solved = true;
            cell.minutes = Some(s.at / 60);
            accepted_at.insert(key, s.at);
        } else if counts_as_wrong(&s.verdict) {
            cell.wrong_tries += 1;
        }
    }

    // First solver of each problem; earlier users in `users` win exact ties.
    let order: HashMap<Uuid, usize> = users.iter().enumerate().map(|(i, u)| (*u, i)).collect();
    let mut first: HashMap<Uuid, (i64, usize, Uuid)> = HashMap::new();
    for (&(user, problem), &at) in &accepted_at {
        let cand = (at, order[&user], user);
        first.entry(problem).and_modify(|best| *best = (*best).min(cand)).or_insert(cand);
    }
    let hinted = |user: Uuid, problem: Uuid, until: i64| hints.iter().any(|h| h.user == user && h.problem == problem && h.at <= until);

    let mut rows: Vec<Standing> = users
        .iter()
        .filter(|u| participants.contains(u))
        .map(|&user| {
            let mut row = Standing { user_id: user, rank: 0, solved: 0, penalty: 0, cells: vec![], badges: vec![], last_accept: 0 };
            for &problem in problems {
                let cell = cells.remove(&(user, problem)).unwrap_or(Cell { problem_id: problem, solved: false, minutes: None, wrong_tries: 0, pending: 0 });
                if let Some(&at) = accepted_at.get(&(user, problem)) {
                    row.solved += 1;
                    row.penalty += at / 60 + PENALTY_PER_WRONG * i64::from(cell.wrong_tries);
                    row.last_accept = row.last_accept.max(at);
                    if first.get(&problem).is_some_and(|f| f.2 == user) {
                        row.badges.push(Badge { kind: BADGE_FIRST_SOLVE.into(), problem_id: problem });
                    }
                    if !hinted(user, problem, at) {
                        row.badges.push(Badge { kind: BADGE_NO_HINTS.into(), problem_id: problem });
                    }
                }
                row.cells.push(cell);
            }
            row
        })
        .collect();

    // Stable sort keeps `users` order among exact ties.
    rows.sort_by_key(|r| (std::cmp::Reverse(r.solved), r.penalty, r.last_accept));
    let mut prev: Option<(u32, i64, i64)> = None;
    for i in 0..rows.len() {
        let key = (rows[i].solved, rows[i].penalty, rows[i].last_accept);
        rows[i].rank = if prev == Some(key) { rows[i - 1].rank } else { i as u32 + 1 };
        prev = Some(key);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn sub(user: u128, problem: u128, verdict: &str, at: i64) -> Submit {
        Submit { user: id(user), problem: id(problem), verdict: verdict.into(), at }
    }
    const P1: u128 = 100;
    const P2: u128 = 200;

    #[test]
    fn solved_then_penalty_and_compile_errors_are_free() {
        let users = [id(1), id(2), id(3)];
        let subs = [
            // user 1: P1 at 10 min after one WA and one CE -> 10 + 20
            sub(1, P1, "compile_error", 60),
            sub(1, P1, "wrong_answer", 120),
            sub(1, P1, "accepted", 600),
            // user 2: P1 at 5 min, P2 at 40 min -> 2 solved
            sub(2, P1, "accepted", 300),
            sub(2, P2, "accepted", 2400),
            // user 3: P1 at 20 min clean -> 20 (beats user 1's 30)
            sub(3, P1, "accepted", 1200),
        ];
        let rows = rank(&[id(P1), id(P2)], &users, &subs, &[], None);
        let order: Vec<_> = rows.iter().map(|r| (r.user_id, r.rank, r.solved, r.penalty)).collect();
        assert_eq!(order, vec![(id(2), 1, 2, 45), (id(3), 2, 1, 20), (id(1), 3, 1, 30)]);
        assert_eq!(rows[2].cells[0].wrong_tries, 1);
    }

    #[test]
    fn non_participants_and_other_problems_are_left_out() {
        let rows = rank(&[id(P1)], &[id(1), id(2)], &[sub(1, P2, "accepted", 10), sub(9, P1, "accepted", 10)], &[], None);
        assert!(rows.is_empty());
    }

    #[test]
    fn exact_ties_share_a_rank() {
        let rows =
            rank(&[id(P1)], &[id(1), id(2), id(3)], &[sub(1, P1, "accepted", 60), sub(2, P1, "accepted", 60), sub(3, P1, "wrong_answer", 60)], &[], None);
        let ranks: Vec<_> = rows.iter().map(|r| r.rank).collect();
        assert_eq!(ranks, vec![1, 1, 3]);
    }

    #[test]
    fn freeze_hides_later_results_as_pending() {
        let subs = [sub(1, P1, "wrong_answer", 60), sub(1, P1, "accepted", 3000), sub(2, P1, "accepted", 3500)];
        let rows = rank(&[id(P1)], &[id(1), id(2)], &subs, &[], Some(2000));
        assert!(rows.iter().all(|r| r.solved == 0 && r.badges.is_empty()));
        let u1 = rows.iter().find(|r| r.user_id == id(1)).unwrap();
        assert_eq!((u1.cells[0].wrong_tries, u1.cells[0].pending), (1, 1));
        let live = rank(&[id(P1)], &[id(1), id(2)], &subs, &[], None);
        assert_eq!(live.iter().map(|r| (r.user_id, r.penalty)).collect::<Vec<_>>(), vec![(id(2), 58), (id(1), 70)]);
    }

    #[test]
    fn first_solve_badge_goes_to_earliest_and_list_order_breaks_ties() {
        let subs = [sub(2, P1, "accepted", 60), sub(1, P1, "accepted", 60), sub(3, P1, "accepted", 30), sub(3, P2, "accepted", 90), sub(1, P2, "accepted", 90)];
        let rows = rank(&[id(P1), id(P2)], &[id(1), id(2), id(3)], &subs, &[], None);
        let firsts: Vec<_> =
            rows.iter().flat_map(|r| r.badges.iter().filter(|b| b.kind == BADGE_FIRST_SOLVE).map(move |b| (r.user_id, b.problem_id))).collect();
        assert_eq!(firsts.len(), 2);
        assert!(firsts.contains(&(id(3), id(P1))));
        assert!(firsts.contains(&(id(1), id(P2))));
    }

    #[test]
    fn no_hints_badge_only_when_no_hint_before_accept() {
        let subs = [sub(1, P1, "accepted", 600), sub(2, P1, "accepted", 600)];
        let hints = [HintReveal { user: id(1), problem: id(P1), at: 300 }, HintReveal { user: id(2), problem: id(P1), at: 900 }];
        let rows = rank(&[id(P1)], &[id(1), id(2)], &subs, &hints, None);
        let no_hints = |u| rows.iter().find(|r| r.user_id == id(u)).unwrap().badges.iter().any(|b| b.kind == BADGE_NO_HINTS);
        assert!(!no_hints(1));
        assert!(no_hints(2), "a hint revealed after the accept doesn't count");
    }
}
