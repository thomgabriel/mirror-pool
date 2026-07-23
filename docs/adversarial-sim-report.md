# Adversarial Simulation Report

- Date: 2026-07-21T18:38:01Z
- Git commit: ac78f9fc1b384619e0ceace48123e57b54fca0ee

## R2 — Whale self-fill sweep (k = 17, worst-first)

| m | effective_k | guessing_advantage | max_funder_share |
|---|---|---|---|
| 17 | 1.0000 | 0.9412 | 1.0000 |
| 16 | 1.0625 | 0.8824 | 0.9412 |
| 15 | 1.1333 | 0.8235 | 0.8824 |
| 14 | 1.2143 | 0.7647 | 0.8235 |
| 13 | 1.3077 | 0.7059 | 0.7647 |
| 12 | 1.4167 | 0.6471 | 0.7059 |
| 11 | 1.5455 | 0.5882 | 0.6471 |
| 10 | 1.7000 | 0.5294 | 0.5882 |
| 9 | 1.8889 | 0.4706 | 0.5294 |
| 8 | 2.1250 | 0.4118 | 0.4706 |
| 7 | 2.4286 | 0.3529 | 0.4118 |
| 6 | 2.8333 | 0.2941 | 0.3529 |
| 5 | 3.4000 | 0.2353 | 0.2941 |
| 4 | 4.2500 | 0.1765 | 0.2353 |
| 3 | 5.6667 | 0.1176 | 0.1765 |
| 2 | 8.5000 | 0.0588 | 0.1176 |
| 1 | 17.0000 | 0.0000 | 0.0588 |

## R3 — Repeated-participation decay (Danezis 2003, per action profile)

### withdraw (N = 100000, b = 17)

- precondition_holds(m=1) = true (m=1 < N/(b-1) = 6250.0000)
- converge_report(m=1) = applies immediately (t* < 1 round)
- precondition_holds(m=3) = true
- converge_report(m=3) = t* = 8.0269 rounds
- seed-distribution summary (m=3, 200 seeds 0..200, max_rounds=2000): success_rate = 1.0000, mean_rounds = 5.5550

### stake (N = 200, b = 10)

- precondition_holds(m=1) = true (m=1 < N/(b-1) = 22.2222)
- converge_report(m=1) = applies immediately (t* < 1 round)
- precondition_holds(m=3) = true
- converge_report(m=3) = t* = 8.8179 rounds
- seed-distribution summary (m=3, 200 seeds 0..200, max_rounds=2000): success_rate = 1.0000, mean_rounds = 7.8150

## R1 — Distinct-funder baseline (k = 17)

- effective_k = 17.0000 (assert PASS: == k)
- guessing_advantage = 0.0000 (assert PASS: == 0)

**RUN PASSED**
