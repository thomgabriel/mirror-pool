pub const ROOT_HISTORY_SIZE: usize = 100;

/// Append a root to the ring (overwriting the oldest slot once full).
pub fn push(roots: &mut [[u8; 32]; ROOT_HISTORY_SIZE], current_index: &mut u32, root: [u8; 32]) {
    let next = (*current_index as usize + 1) % ROOT_HISTORY_SIZE;
    roots[next] = root;
    *current_index = next as u32;
}

/// A root is "known" iff it equals any non-empty slot in the ring.
pub fn is_known(roots: &[[u8; 32]; ROOT_HISTORY_SIZE], root: &[u8; 32]) -> bool {
    if *root == [0u8; 32] {
        return false; // the zero sentinel is never a valid root
    }
    roots.iter().any(|r| r == root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> ([[u8; 32]; ROOT_HISTORY_SIZE], u32) {
        ([[0u8; 32]; ROOT_HISTORY_SIZE], 0u32)
    }

    #[test]
    fn seed_root_is_known() {
        let (mut roots, _ci) = empty();
        roots[0] = [5u8; 32]; // non-zero seed (the real seed is the non-zero empty-tree root)
        assert!(is_known(&roots, &[5u8; 32]));
        assert!(!is_known(&roots, &[6u8; 32]));
    }

    #[test]
    fn pushed_root_becomes_known_and_recent_history_survives() {
        let (mut roots, mut ci) = empty();
        roots[0] = [9u8; 32]; // non-zero seed
        push(&mut roots, &mut ci, [1u8; 32]);
        assert!(is_known(&roots, &[1u8; 32]));
        assert!(is_known(&roots, &[9u8; 32]), "recent history still valid");
    }

    #[test]
    fn zero_is_never_a_known_root() {
        let (roots, _ci) = empty();
        assert!(
            !is_known(&roots, &[0u8; 32]),
            "the zero sentinel is never valid"
        );
    }

    #[test]
    fn old_roots_evicted_after_ring_wraps() {
        let (mut roots, mut ci) = empty();
        roots[0] = [200u8; 32]; // distinct non-zero seed
                                // push ROOT_HISTORY_SIZE fresh, distinct, non-zero roots so the seed falls out
        for n in 1..=(ROOT_HISTORY_SIZE as u8) {
            let mut r = [0u8; 32];
            r[0] = n; // 1..=100, all distinct and non-zero, distinct from the seed (200)
            push(&mut roots, &mut ci, r);
        }
        assert!(
            !is_known(&roots, &[200u8; 32]),
            "root older than the 100-slot window is rejected"
        );
        let mut newest = [0u8; 32];
        newest[0] = ROOT_HISTORY_SIZE as u8;
        assert!(is_known(&roots, &newest));
    }
}
