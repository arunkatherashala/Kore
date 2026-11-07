# Digital Life (kore-self): what was built and what was measured, B1 to S4

Status as of 2026-10-31, branch `main-Digital-Life-AI`. Every number below is copied from `docs/measured.md`, which has the run details, seeds and commits. All numbers come from synthetic worlds and sequences written by the same author as the learners. None of them is a claim about real-life accuracy, and none is a claim about intelligence, consciousness or being "alive".

## What exists

A local, deterministic system with an append-only, hash-chained, versioned log (events, revisions that cite the events that caused them, predictions, outcomes), SQL over any past version, ingestion, forgetting, and an enforced `authorize()` choke point. On top of it, a research track on a being that learns only from its own logged experience in a sandbox world. No pretrained language model is the mind.

## Results by stage

| Stage | Question | Result (synthetic, seeds 1..20) | Verdict |
|---|---|---|---|
| B1 world-model learner | Does learning per-option success and cost from its own log beat simple choosers? | Beats UCB1 by about +1.5 reward per energy at budget 600 (interval +1.2 to +1.8, better in 20 of 20 worlds); within 0.38 of the oracle at 600 and 0.16 at 3000 | Works in this designed world. Most of the gap comes from picking a strategy and skipping bad menus, which the baselines cannot do. |
| B2 rule discovery | Can it find new rules by verified search inside a 17-form grammar? | Discovery minus no discovery: +0.162 [+0.000, +0.324] at budget 3000, 8 wins, 7 losses, 5 ties. 99 of 99 accepted rules correct, 0 accepted on the two families outside the grammar. Recall 62 of 120 (51.7%). | Safe (no wrong rules) but the reward gain is not established. Recall is about half. |
| B3 adaptation | Does forgetting plus change detection help when the world changes? | Adaptive minus B1: +0.030 [-0.181, +0.241], 10 wins, 10 losses. Forgetting alone: -0.100 [-0.124, -0.077], worse in 20 of 20. | Negative result. The tuning itself chose almost no forgetting. |
| B3b adaptation, second attempt | Why did B3 fail, and can a learner lose measurably less after a change? | Diagnosis: the headroom after the changes is failed exploration of hard cells with optimistic priors, not a hidden change (told the true degradation, B1 gains at most +0.03); attempt 1's alarm response and its cold-start gain were artifacts. New learner (window switch + prior-conflict check): post-change windows +0.095 [+0.043, +0.147] vs B1, 16 of 20 worlds better; whole life +0.027 [+0.010, +0.044]; stationary +0.003; new-domain competence 42 energy units sooner. Not established against attempt 1 or, on the tuning seeds, against the fixed policies' excess regret. | Weakly positive against B1 only; most of the headroom (+0.52 told-what bound) remains. |
| B4 advice | Does an outside advisor help, and can trust be learned from outcomes? | Following advice blindly loses below an advisor quality of about 0.93 to 0.97. The trust learner is never significantly worse than no advisor. Good advice mostly speeds up early learning. | Trust learning is a useful safety property. A real model was not tested, so real advice quality is unknown. |
| S4 predictions | Can a ledger score predictions written before outcomes, with honest baselines? | Context matters: context-count beats frequency in 20 of 20 seeds, Brier gain +0.3936 [+0.3802, +0.4081]. Pure noise: no predictor wins in any seed. Drift: recency beats frequency in 20 of 20 seeds, gain +0.1937 [+0.1829, +0.2040]. | The machinery detects structure when it exists and does not invent it when it does not. Each predictor wins only where the generator matches its assumption. |
| B5 composition, pruning, controller | Can it build rules from verified blocks, retire stale rules, under hard owner limits? | Composable families found in 20 of 80 pairs (B2: 0); reward change +0.006 [-0.008, +0.019]; 0 wrong rules; pruning effect small and on few worlds; caps never exceeded; kill switch stopped 20 of 20 at step 40 | Safe and bounded; composition enlarges what is found but no reward gain; pruning not shown to matter |
| B6 transfer | When it has learned in one domain, does that speed up learning in a new one, and can it avoid transfer that hurts? | First 100 steps after the new domain appears: +0.211 [+0.054, +0.367] reward per energy, better in 11 of 20 worlds, worse in 0, 9 ties; reaches 90% of the oracle's rate on the new domain in 8 of 20 worlds against 6; wrong cells cost -0.20 [-0.36, -0.04], the guard recovers +0.18 [+0.06, +0.31] but does not remove the harm; 0 wrong rules | Small early gain from links; search order no effect; gain may be partly early exploration; most of the oracle gap remains |
| B9 experiments | Can it choose tasks on purpose to test its own rule hypotheses (expected information gain plus missing support, within an owner-set energy budget), and does that make it learn rules faster and earn more? | Tuned setting (5% of the energy): more rule targets covered than passive discovery in 10 of 10 conditions (interval above 0 in 8; World v2 large clean +7.35 [+5.53, +9.17], B2 budget 3000 +1.70 [+1.02, +2.38], B5 +2.85 [+1.61, +4.10]) and covered with less energy in 9 of 10; 0 wrong rules in every arm. Reward per energy vs passive not better anywhere (point estimate negative in 8 of 10; B2 budget 600 -0.144 [-0.282, -0.006]). Beats random probes with the same budget on coverage in the menu worlds and large World v2 worlds, not in small and medium World v2 | Learns rules faster and safely; no reward gain (negative result for reward), because the planner barely uses rules. Details: measured.md, Track B9 |

## What went wrong along the way (kept on purpose)

- S4: the first uniform baseline gave no mass to an `(other)` choice, so every learner "beat" it on pure noise in 20 of 20 seeds. This contradicted the noise scenario, was found, fixed, and the first numbers were not reported.
- B2: a first guard accepted 4 wrong rules (consecutive primes fit a lag-2 rule). It was tightened to 3 spare equations, which cost recall.
- B3: the planned adaptive learner did not beat the plain one.
- Engine: a recursive LIKE matcher overflowed the stack on about 10 KB of text, which crashed the server. It is now iterative, with a test.
- A deadlock in a B4 end-to-end test (the being run held a lock that `authorize` also took) was fixed with a locked variant of `authorize`.

## What is not known

- Anything on real data. The live prediction ledger has not been run on real data.
- A real model as advisor or thinker (B4 used a simulated advisor).
- Behaviour on very large logs, long unattended runs, or crash recovery.
- Sensitivity to the synthetic generators (sequence length, number of choices, strength of structure, drift period).
- Whether any of this generalises beyond worlds the author designed.

## Honest reading

The solid, reusable part is the body: a verifiable, versioned record with provenance, safe tools, and a scoring harness that refuses to claim wins that are not there. The "mind" results are narrow: a learner that finds a good policy in a designed trade-off, a rule search that is safe but not clearly profitable, no gain from adaptation. These are research findings, positive and negative, not evidence of general capability.

## Next steps

1. Run the prediction ledger on real, opt-in data. Value comes from elapsed calendar time, so it should start early.
2. Test a real model behind the advisor seam, with the trust learner, against no advisor.
3. Add crash-recovery and large-log tests before relying on the log for anything long-lived.
