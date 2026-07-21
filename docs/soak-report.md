# Soak Report

- Date: 2026-07-21T14:34:46Z
- Git commit: 24d1cee356a6edcb4c92e0f47d273afd55d77000
- Program ID: 7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3
- Validator version: 3.0.1

## Phase timings

| Phase | Duration |
|---|---|
| preflight | 3.66s |
| setup | 1.52s |
| withdraw: deposits | 9.69s |
| withdraw: prove + commit_intent (k = MAX_K_WITHDRAW) | 397.00s |
| withdraw round (k = MAX_K_WITHDRAW) | 428.39s |
| stake: deposits | 5.08s |
| stake: prove + commit_intent (k = MAX_K_STAKE) | 211.94s |
| stake round (k = MAX_K_STAKE) | 236.11s |

## Transactions

| Label | Signature |
|---|---|
| setup: create_validator_vote_account | `665GY8xZN3wZcWKtYS6x4byB1uTw67Kbb8unQWcx7RLB4GdAc5g3CcQyLdvS72wTCws5LLw45ywvMoWgN6EQxSCT` |
| setup: initialize_pool(withdraw) | `5Uyh2iUQbQ9VpeonPCzFksynCjWtSk9JMsB8YZcCWtfxTKdkW8v6ku8WFPNrzvNGuMVE9rHzB7sVNr67zx1C81EF` |
| setup: initialize_pool(stake) | `6fawwEzPn4dTSGjb71WrEsf9gXpCzUj5PtBkJ2HYmnEdbLn7ti34VanZEbqD2Xo5iCFAmKbiSJFjfromTJ3y3fg` |
| withdraw: deposit[0] | `33R3TtcgBop7smaM7s2fZMXArU7dE2YrDnbZNTamUev3xr6ZK6DyTn4pEcExSmt1vXvN2bvX3sD7Rmu4s8ue9oHf` |
| withdraw: deposit[1] | `5cBpTuvjCbXj2kZFVTACbx13nM56FE9eKTa7kPWph2f2LDAnMq7fbpKbSEyfZUtRZAagCfZYtyGGNixW5wP5UP1z` |
| withdraw: deposit[2] | `3EgLPomgLtRAtq3MrgVzZFhgNw5GxVnVSu5Jk5PmM8ehindbxYBjvFMxd7yXAoxczRjWu7KNVfQyQZABjD3t4k1J` |
| withdraw: deposit[3] | `2i6tPRw9wV4q24D3iwusPj6KHgCGTL1bwQ4bEE3ouMsAeouHKUQm9HsoeuH2XFC43b64PpCQJpuGbc5pfsuCLtYs` |
| withdraw: deposit[4] | `2v7ra5n9e8Rx11MTKTLEMQVGRU4dg7RhnFzFGqx3w7rCrJxYbTV6WPM6TnCeXA1iiExd1wmiR5Ps3Rj8QvLSS5dr` |
| withdraw: deposit[5] | `qSvimtQhdjgP5GY7xcZU6fG2RWrXXoDdYEEGxtxZiUH2jR2XpeAqe3XFDAdc6cCkCmQmakpBApohcbz2vwgw2sx` |
| withdraw: deposit[6] | `2q1iq5eJ1gA14uj8Q3TUXNqA4CH7H5qcdDL5ZQ2A9gfBiQQJwJ6ynh4WNWU4123WxunyDPcbM8iCUyhzRdf2erCJ` |
| withdraw: deposit[7] | `4BAKY3PSuKeCFXkA7D5Bra8E9feaN7H74AYa6315CMsLdNRnaJTbmDKG8DCfQt1TEPjx2auzjhq2jNpxaFazAjj5` |
| withdraw: deposit[8] | `3ieEKBKSCMP6f5AjfMvS35kf9ufqD2zxRbCYkZwrk8e4PRbBistRBCP5TgGWLH6YwAsz1JzLxVX4dtYF7wCdAQSM` |
| withdraw: deposit[9] | `5trKA1Zq9DZWHbHQSW5gRjKXnHXahDqE7ZS7mpEjUDcSCwbaxY9suqkJF5kBCaPs6Xn3WYqM6kyEWeyPo1c2nTnp` |
| withdraw: deposit[10] | `TGpDxVVctuhbYaN6mNKSCToQKWosPFFrtJVspTBLxMCYingZ9TLD6A59sZ3Tw7JZCLy1CY1RdswKKA1vZAHt1TJ` |
| withdraw: deposit[11] | `QTpQTgT7PgZsM2RPLksMzg7Vnmzxv49tGamVS5rG4NcxZNj5rkyB5M6jnJQexp74XuxtKNC87q9sFPbABm48SWm` |
| withdraw: deposit[12] | `3UYKduBF1ew8BnCV8xvRcX8UoodPXhWvihyfgtbhP49ZiA4xMmxU81HrQ6DmxVQpU4QmNbLqQzZCPLhpac1AQN8U` |
| withdraw: deposit[13] | `3gRhsDGYe7udwBiE8KP3b3twByRWdptGs3vPHn499NNZ8xSFm9cgsgYNtGNfvnR3ojyySJT4GqGR4tdKTxEUHws8` |
| withdraw: deposit[14] | `3LcFWsk5rmM2eN6maEzRWSekhth6qMf4QJ2ZyFVcazCD9BzsLqc7CADa7FKuXCiFypviFpckG25ZxeCs5MUAAWPR` |
| withdraw: deposit[15] | `48uYR8R7pViYF8NnrHsZ6bmaTdnWK1SpooPzcQATDpwJRtA9eU7g1FFJmavkST2fDc5byuCZ7heDjv865eqvqBRY` |
| withdraw: deposit[16] | `3QTg6ExQR4hNinPYTzW9KDwhxrnDD5QPKc1s9JsHx2zU5M7Nk2QgWa4ESm9nvoJJhRYoua4h8vTGSj75mZaPSG9y` |
| withdraw: commit_intent[0] | `5GE5FumgnhVeo17yjXMYn1QGo5evDdckKPPmY1idYLJay6UNQw3JfcDsLHomdrVWuuQYzGZDLoX6MupgDn3rc348` |
| withdraw: commit_intent[1] | `5FwUfdv8j6DaTPw24dyzgFLtyKLHmiuY2kNndrzqXzwPXZGXYcYc8jkk55A4TUMCjF5Mt8ToRANo3bD9YgaTGeJd` |
| withdraw: commit_intent[2] | `5FEjSwkBCRi651vg2N3Qe16A3FPcrJx5LheiTDeByfSwcD1EqTh2ZNs7d6TRTS9LNXMqyk3QaUxK4JF4zssgFRpd` |
| withdraw: commit_intent[3] | `3FQvUThYi2acxWpHVsPnaio896o3b5yRYJ2VzqBDX8FZKSj6qcd7USnzjwPQqo38mPM2LJLrz1Z8h42Lsqk9n7fh` |
| withdraw: commit_intent[4] | `4fkG9az3CGvVfxe5SAXEzaRsRcYFZdjrX61WLWjXfcyi8tXR6qrVWpfcBsVJzBAPqUq2rHafnP1pUAWByt9sF5Qu` |
| withdraw: commit_intent[5] | `2Ep212LCSCcxumdco7QE3dMMPWsCAzybTiUFWDq2ws7z4m35kM2waUDX4M75CY1RfRRm5CiHLSDdSpkq1ejth38w` |
| withdraw: commit_intent[6] | `2RrddzWYVjscjtXk3bdUYyz37eSdy4uBqGSgzHLDo9W9M13kZaEqPm5oaZ9ANyT5Navjxc2XcJaRsA83XGiCAKv9` |
| withdraw: commit_intent[7] | `4scjTQsidjnbXmpFCYga51o8SKLJnuaV9qiLiyz9rKQRqkDDMNiApra7yWPeDsA3EKa7w9Brjx4ktUfRHEJXtnga` |
| withdraw: commit_intent[8] | `T97iF9Hda27nKQ39ipjXtC7N6GK67GaG34Ak4JdM3ckGsYGuLBmLKR1ja5eYCfcdyGawn591gyA8G16ikfisDXp` |
| withdraw: commit_intent[9] | `3hRptVVmUCNnCcjTychi66nEYgr5JZgPigBWTw3CqUGZ2MMuZfc9ib7SeZi2E9M1MF1azNspXgBVLAF4J3TSkkGN` |
| withdraw: commit_intent[10] | `4M4vrKLWhrsRg2vgUZH9qvsXmhzzg5jyG5DuH2vyENvPFKQcxbFXZK2VuHSWW4ren4stYY1khC1gpaEFpyHGbs1z` |
| withdraw: commit_intent[11] | `2B6c1faaEYbiNS1JmnXmsKaMy1XihhfQE44oAcAoGdp5CdqAGwWwSCfMLLV2DJNCFctV3JhAXGfDbYoxwYY85vN` |
| withdraw: commit_intent[12] | `4YBcBBzDAToRKHxDbQVBGf8L1zkCH5bv9NbacCt5LjULGjjBZ96eLm5uGnKAmLjW2rjTengY5Gc5kg2KQMQUHsJS` |
| withdraw: commit_intent[13] | `2qUobMYbkTjcuznkHdbEkWecZiB813cQ4KL6N4MvaEBVRvhdfL4k5qXSRurr2Sh24xqifgUAQNViXiAd5YiLfSmK` |
| withdraw: commit_intent[14] | `2VxKfmtfMBcGjRQ5YFXwuEkmP5QyyFuBx1uvgwF6DUmwQYWFA76aSYWB2GZSGDNHqKoUmNqCo4T1N4g3i8CpNfi7` |
| withdraw: commit_intent[15] | `3Qw8j34KLHWCHMq9hLC6pR97ASCjhHM9DMnjAtrjFdkUCQspYZuy3M16yxQqSMpFrtx2jLM4wcKH8Fr1zWgeM6DD` |
| withdraw: commit_intent[16] | `ksvBFRdVveBJ4x7hjJCoshgiSczofgfV3w84f59hfvhDAr6Y3qC8KyNNzNLsUYc4FvNg1JpEMeEcTN52AtKz6tX` |
| alt: create_lookup_table | `5Y9uZhD5F9va3M2mGJMxFk1B4174uq7pqC7rA2sH3LVCkdWmZuNThJbfjuFCcr4WToRqf2GAXRKFuPjAsqsBb75V` |
| alt: extend_lookup_table[0] | `47jBTLnYbtJrxFaFDrHHU3uUkjrVKA7PgdofcoEoeMvwHQhTg45W69CjjZehCbUbECG6ssn2wGTM7AV3f9nAga89` |
| alt: extend_lookup_table[1] | `StJ1yDePAKTsZzEfmm5rfDua7fNbmshP7MDFMnGjYJkaihNdmRzPk56vyM6irMyFbp1tYXd6eqa7w1GVnc47Ha9` |
| alt: extend_lookup_table[2] | `3dPNj6YL6yTx6XLMpcPMXG7T7VSpertaYFT9iVxMdfeATMj4xRG3JxjFdXgyvhBVxW9z7cg3hmt2XxqpdJoF3SKy` |
| withdraw: execute_round | `2K495pcnd391FDcu5MbJxro14gT7UxgbDiTB4wYY3HS3pDLBRzefJDRWeccamSx7e2x1ecRwidNR95Dd6oomjq1x` |
| stake: deposit[0] | `236oS1ihvNCzs8G8Lo8uGoZSJWyMjHFxsL6ciMG7BHwt8UbwYY8o7iNuuM36X8a88EBUYxad4p7NHbRGFhB9xA2p` |
| stake: deposit[1] | `3JsgnEEMQVnyhfByN8eNz5LgSWcavHouftQWJKwTX6Uj1qDYvaodJWtcPbmVYJDEqUa45rKqEkDGkFWPYeKo5pT7` |
| stake: deposit[2] | `zovHVXtVg7y7qakrdi7eTzzq8NdhapswzXZJZ5pVPuSWunDhpD1VUJBK9zTj1KDrnSjoSQLVWSxGqQsVdAEHaVS` |
| stake: deposit[3] | `21QvAJNQJDKh4MFAGdikvFzBJ3n29TCwhBwA76M8PDSTHYggXgSVNMfkxoByRZFs7qixdCEhNrkyDCDac6fkHYXx` |
| stake: deposit[4] | `5xRWWRPKbFZ8yHrgaKZ4pjBfpw2Hnr2kqpovV6rViSeH7pbioLAFnvqRH7fEFiiNtu7pvxBSULuc5TJg6z3qNodU` |
| stake: deposit[5] | `2sMUEoEuv2YRNR6DWFDqn4SWPso8pYzi4z4xU753hzJqY2VK2UoQ9oAGzka9tfmizSY33NbEG3M6NVPAnjU6U3wY` |
| stake: deposit[6] | `3pTenPYt1eDvzCeLdEYkxtm42FR2EZNqNAyXZ1d8dK4bEC7JxQenEMcQRosdV7dhsFPyDBVyz242sgMt7qUp4Myz` |
| stake: deposit[7] | `GvJpYY1jqASzRXK3iDRgDWRJgVv1cMbRjunEhFuW9brJA6vHuyMRhkjDAPkgpbahX13o7u26iUAWoKHZxsfYALy` |
| stake: deposit[8] | `46MtK7XvY9ihqn53vRWvsS2H1no625kN8QwmT6hD3omXbZyRC7nC31zfQopTcf7iw4R2nK5np14sqMzF6jdaNqLb` |
| stake: deposit[9] | `asr2ggHSzKxFHh4FBVjNHL8RjE5yCYY1SR5PR7YaLeWskndGKgM282fbNaJZdjGNx1ARUZSGqrZ2j7PYTMibGRR` |
| stake: commit_intent[0] | `2B6Gt3zzcwPTbFs3N18F63ZSD94J5xLfQJrgKfXqBDng3i3RFLiu6ZdGANSApSBoHemSNafB5MyDkz32CYxVrnS3` |
| stake: commit_intent[1] | `4AmYEvsdXAAcDRXP85P5SLnpAKKrA3sCemGoyNjhSRoJeZ8ALqDccH9AMNnGn9fu3qv139rZms7WxmnJZmiEJHS7` |
| stake: commit_intent[2] | `3tRpjrS5KucngZKV88hu1w8DCwBYbJwTRe9mhMuMd7h1xfNoPbBzMqha9owtuStm41fjD6NCSdc4sG7mHYZV7Tby` |
| stake: commit_intent[3] | `2Qk9hYuQ7oL23RFYJws1mRG91zdYBamS13RQW5BYinw344sBUxfZ6kpdNQ3cnVsRPzbKz5mqTWYcbcw7fzgScYEL` |
| stake: commit_intent[4] | `4ukXYaDdhcYkgfSeeVCgftu3CWhU6djUvhjEZ7ngKQ4X3bJkMrJEurS9MtysFC8daHKMb3vppDTdSyunu533c3Xc` |
| stake: commit_intent[5] | `3HVB23kvoRCCsWC7VMUPPF9gBw5czAVWaYvyRxiv5Q9qPHo1viGM9FuDUx76fzTxRr5pTdmq6rr8FmLAkaY52S13` |
| stake: commit_intent[6] | `2oF5q3mjHsC8rZHB22B1Zjh2U9ssCZzGq5c653mnV62UpjnwvQBtceK8FkUAWHYGrmBfPoWmZCa2o9Cpqj2gA2z4` |
| stake: commit_intent[7] | `4qKCEMUct3ESEDXaJHKTR75jkSLDcCfxfvfr7uej6xi8pJsYDxZ2xuGAf4MHpebDrm2R1H7EoTsYs82XYmhCUz3b` |
| stake: commit_intent[8] | `Ab6PQbWkqSCVUyuG3yPorRVUnh8S3JibH2r2B9EqcGxfZow8DLJyqZdpuSU5HzSSMUQLmTU2TDFjygDwmiUupEC` |
| stake: commit_intent[9] | `22LhENBN9oK1CUoFVmsFyyzYLYoL1xCBnevBret6K9Yj5sG43H5d6RBr5Apsx9UtU36PQ52vNCzFU2pQ41fVG5r3` |
| alt: create_lookup_table | `46Up2Xv9At1QH2kJ1fW5xvEhxMWhCoLWcyMqNVtgHCyhUmE8iNeY8H6MP6saDXiWc7v5r33SubuAj6STiJUHCZvr` |
| alt: extend_lookup_table[0] | `5MnuVzbEfU2C5Eha7Ba2oegKKD4bAi7KCKmefVfKJ4tBsoAN2TDuCtMZqi1J5Ga2zAVMR2egY1nMw1vFEaRZpqAZ` |
| alt: extend_lookup_table[1] | `4sYg2SUWmgHomASBdRTbas522fzdm9Wxwroycn24Q8tiJiZ4ZLeas3ZHGn2wGVpPnPcfWyrrsadToZn1EUENbfHB` |
| alt: extend_lookup_table[2] | `5N6Kj1PGxZ3v42MXdRhLtQdbQwbs9rAwqBXQW5SqbqVsyMAvcS2goTZoAdBKGaUJNnwQFiDrN4x3VqUJvpo2FueF` |
| stake: execute_round | `2e5qJ2JgLSme1wXJ5Hok7KDStqZTh7EMoPWqU1C9koUjjuDGEmjRGMKrFLMcY6JkVwhTVvyeeKzyn85i1v1jXMH8` |

## Assertions

| ID | Description | Result | Evidence |
|---|---|---|---|
| A1 | execute_round's on-chain signer set is exactly the operator/cranker — no recipient, relayer, or depositor signs — and every forbidden key resolved into the transaction via ALT (the joint uniform-actor property) | PASS | signers = [AWa7EqxA1uD5SBTxpyyZGXpi2gRzFxtyK5Tj8PLLQKKu]; sole signer + 34/34 forbidden keys present-but-unsigned via loaded_addresses |
| A2 | vault balance drops by exactly k * denomination across execute | PASS | pre=1700890880 post=890880 delta=1700000000 expected=1700000000 |
| A3 | every recipient/relayer credited exactly its uniform bucketed amount | PASS | 34 accounts checked, all match: recipients: 17x 99000000 lamports; relayers: 17x 1000000 lamports |
| A4 | all k nullifiers spent (single-spend) and the duplicate-commit probe fails without mutating existing PDAs | PASS | 17/17 nullifier PDAs present & pool-owned; duplicate commit_intent probe: send_failed=true, intent_pda_unchanged=true, nullifier_pda_unchanged=true |
| A5 | executed round is Executed; the next round exists, Open, intent_count=0 | PASS | round0.state=Executed; round1.state=Open intent_count=0 |
| A6 | live effective-k, computed by crates/effective-k from the run's true funding composition (reported, never gated) | PASS | AnonymityReport { nominal_k: 17, effective_k: 1, shannon_effective_k: 1, guessing_advantage: 0.9411764705882353, max_funder_share: 1 } — EXPECTED AND DISCLOSED: a solo operator funds every note in this soak, so this is the maximal-whale case and collapses to effective_k=1.0 by construction; a real deployment's effective-k depends on independent funder clustering, which a solo run cannot exercise (see docs/SOAK.md). |
| A7 | resolved account-key count (static + ALT-loaded) <= 64; compute units consumed recorded | PASS | resolved_keys = 3 static + 56 loaded = 59 (<=64); compute_units_consumed = 114135 |
| A1 | execute_round's on-chain signer set is exactly the operator/cranker — no recipient, relayer, or depositor signs — and every forbidden key resolved into the transaction via ALT (the joint uniform-actor property) | PASS | signers = [AWa7EqxA1uD5SBTxpyyZGXpi2gRzFxtyK5Tj8PLLQKKu]; sole signer + 10/10 forbidden keys present-but-unsigned via loaded_addresses |
| A2 | vault balance drops by exactly k * denomination across execute | PASS | pre=10043719680 post=890880 delta=10042828800 expected=10042828800 |
| A3 | every recipient/relayer credited exactly its uniform bucketed amount | PASS | 20 accounts checked, all match: recipients: 10x 1003282880 lamports; relayers: 10x 1000000 lamports |
| A5 | executed round is Executed; the next round exists, Open, intent_count=0 | PASS | round0.state=Executed; round1.state=Open intent_count=0 |
| A6 | live effective-k, computed by crates/effective-k from the run's true funding composition (reported, never gated) | PASS | AnonymityReport { nominal_k: 10, effective_k: 1, shannon_effective_k: 1, guessing_advantage: 0.9, max_funder_share: 1 } — EXPECTED AND DISCLOSED: a solo operator funds every note in this soak, so this is the maximal-whale case and collapses to effective_k=1.0 by construction; a real deployment's effective-k depends on independent funder clustering, which a solo run cannot exercise (see docs/SOAK.md). |
| A7 | resolved account-key count (static + ALT-loaded) <= 64; compute units consumed recorded | PASS | resolved_keys = 3 static + 41 loaded = 44 (<=64); compute_units_consumed = 232026 |
| A8 | each stake account's FINAL state (not Initialize's params): Stake-program-owned, authorized staker/withdrawer handed to recipient, delegated to the pool's validator | PASS | 10 stake accounts checked: all Stake-program-owned, authorized.staker == authorized.withdrawer == recipient (Authorize(Staker) handover landed), delegation.voter_pubkey == vote_account 7sWmY1Xa8L9Me1d1h1knMFbHgnJR28gbdsJApRo66a5b (delegation state only, activation not asserted) |

## Notes

- setup: vote_account=7sWmY1Xa8L9Me1d1h1knMFbHgnJR28gbdsJApRo66a5b withdraw_pool=5WstdBVsNhnZFcTN5JwKvU9fD9BPzHKWvTKLn6suxFcv withdraw_vault=Fc3ynWhYaVJzu3xrqXS9D2jVerjgd8UGb5gM7GAmQt7Z stake_pool=HgMLtRGcoQh3WM92F3f9pSzxfrypjMsQ4uract3JYeJh stake_vault=8CN5wnehx7Mt9TsZsWjoRFWL93vPp2BadzcPGxTkMq6P mints=(6Yzj5ydq9pVrWzp896ueToCcUgQeduWoDGF2RVHcwiqK, 3cKckTT2VYwAZi8RZ7VucCLdfcTr127ni6vMNzkEX2hp) stake_denomination=1004282880

**RUN PASSED**
