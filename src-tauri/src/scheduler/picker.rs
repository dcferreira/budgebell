//! The rotation picker (design spec §4.3): weighted, but deterministic given
//! an injected seed, and it must never repeat the immediately-previous pick
//! (unless there is no other candidate — a rotation of one always repeats,
//! since it has nothing else to show).

use super::ids::HabitId;

/// Deterministically draws one member from `candidates` (habit id, weight),
/// excluding `previous` where possible. `seed` selects the draw via
/// `seed % total_weight` against the candidates' cumulative weights, so the
/// same seed and candidate set always yields the same pick.
pub fn pick(candidates: &[(HabitId, u32)], previous: Option<HabitId>, seed: u64) -> HabitId {
    assert!(
        !candidates.is_empty(),
        "a rotation always has at least one member"
    );

    let eligible: Vec<&(HabitId, u32)> = candidates
        .iter()
        .filter(|(id, _)| Some(*id) != previous)
        .collect();
    // Excluding the previous pick would leave nothing to choose from (a
    // rotation of one) — fall back to the full candidate set.
    let pool: Vec<&(HabitId, u32)> = if eligible.is_empty() {
        candidates.iter().collect()
    } else {
        eligible
    };

    let total_weight: u64 = pool.iter().map(|(_, weight)| *weight as u64).sum();
    let mut roll = seed % total_weight;
    for (id, weight) in pool {
        let weight = *weight as u64;
        if roll < weight {
            return *id;
        }
        roll -= weight;
    }
    unreachable!("roll is always < total_weight by construction")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LUNGE: HabitId = HabitId(1);
    const GLUTE: HabitId = HabitId(2);
    const WALL_SIT: HabitId = HabitId(3);

    /// Design spec §4.7 example B's weights: Lunge-and-reach 2, Glute
    /// bridges 1, Wall sit->squat 1.
    fn example_b_candidates() -> Vec<(HabitId, u32)> {
        vec![(LUNGE, 2), (GLUTE, 1), (WALL_SIT, 1)]
    }

    #[test]
    fn the_previous_pick_is_excluded_even_when_it_has_the_highest_weight() {
        // Given the previous pick was Lunge-and-reach (weight 2, the highest)
        // When picking with any seed
        for seed in 0..10u64 {
            let picked = pick(&example_b_candidates(), Some(LUNGE), seed);

            // Then it is never picked again immediately
            assert_ne!(picked, LUNGE, "seed {seed} repeated the previous pick");
        }
    }

    #[test]
    fn the_pick_is_deterministic_for_a_given_seed() {
        // Given the same candidates, previous pick, and seed
        // When picking twice
        let first = pick(&example_b_candidates(), Some(LUNGE), 7);
        let second = pick(&example_b_candidates(), Some(LUNGE), 7);

        // Then the result is identical both times
        assert_eq!(first, second);
    }

    #[test]
    fn the_weighted_pick_matches_the_cumulative_distribution_exactly() {
        // Given Glute bridges (weight 1) and Wall sit (weight 1) as the
        // remaining eligible pool after excluding Lunge-and-reach
        // When picking with seed 0 and seed 1 (total eligible weight = 2)
        // Then the exact picks match the cumulative-weight layout
        assert_eq!(pick(&example_b_candidates(), Some(LUNGE), 0), GLUTE);
        assert_eq!(pick(&example_b_candidates(), Some(LUNGE), 1), WALL_SIT);
    }

    #[test]
    fn a_rotation_of_one_always_repeats_its_only_member() {
        // Given a rotation with a single member, previously shown
        let solo = vec![(LUNGE, 1)];

        // When picking again, excluding the previous pick would leave nothing
        let picked = pick(&solo, Some(LUNGE), 42);

        // Then it falls back to repeating the only member
        assert_eq!(picked, LUNGE);
    }

    #[test]
    fn with_no_previous_pick_every_member_is_a_valid_candidate() {
        // Given no prior pick (the very first tick)
        // When picking with seed 0 (falls at the very start of the
        // cumulative distribution)
        let picked = pick(&example_b_candidates(), None, 0);

        // Then the highest-weighted member (first in cumulative order) wins
        assert_eq!(picked, LUNGE);
    }
}
