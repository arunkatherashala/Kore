# Track V1: the being versus standard methods and a language model (synthetic worlds, author-defined)

Question: where does the Digital Life being's learner (crate `kore-self`, learning only from its own logged experience in sandbox worlds) stand against well-known methods from the literature, implemented cleanly and tuned fairly, and against a language model playing the same worlds? Same worlds, seeds, budgets and scoring for every non-LLM method; paired differences on identical worlds with 95% t-intervals and better/worse/tie counts.

Everything here is the author's simulation: the worlds (World v2, World O, World L, the first menu world with the B2 pattern families), their families, rules, noise, drift and rewards, the being, the implementations of the methods and this measurement. No real data. Nothing here is a statement about general ability; it says how these learners compare in these worlds.

Summary (details and intervals below):
* **Where rules matter, the being is clearly ahead.** World O (open-ended, 600,000 energy): the current learner beats every standard method by 0.54 to 3.10 reward per energy (best method Thompson, +0.540 [+0.380, +0.701], better in 20 of 20 worlds). Rule learning on the same revealed examples: the verified B2 search answers 78% to 81% of fresh sequence tasks right and none wrong, against 59% to 71% for brute-force enumeration in the same DSL and 5% to 55% for k-NN and regression trees (which answer everything, so the rest is wrong).
* **Where rules matter little, the being's choosing part is behind a tuned linear contextual bandit.** World v2: LinUCB beats the current learner by 0.18 to 0.38 (11 of 12 conditions significant); the recommended profile (B7 rule use + B11 skip fix) only ties it (-0.09 to +0.13). First world with the B2 families: LinUCB is 0.45 above at budget 600, level at 3000. World L: the one-step planner is 0.07 to 0.21 below the best bandit in every setting; the B8 lookahead wins only in the unlock setting (+0.34 at budget 1600).
* **Against the references that see hidden information** the being is 0.19 to 0.57 (World v2, recommended) and 0.52 (World O) below oracle-knows-rules.
* **The language model** (haiku, 3 worlds of about 11 steps, 31 calls, median 71 s per call) gives no measurable comparison; it answered 9 of 9 sequence tasks right by itself.

Code: `kore-self/src/being/vs_world/` (new: `core.rs` the methods, `adapt.rs` the world adapters, `mcts.rs` the reference with the true simulator, `rules.rs` the rule learners, `llm.rs` the LLM player, `tests.rs`), the harness `kore-self/examples/measure_vs_world.rs` with `measure_vs_world_parts/llm_run.rs`, and the wrapper `kore-self/examples/llm_advisor/claude_player.py`. Existing files touched: one module line in `being/mod.rs`, and a one-line merge fix in `world_o/engine.rs` (`LearnerOCfg::w2` lacked the B10 `skills` field after the brain-v5 merge, so the crate did not compile at a4ca4231; it now fills the field with its default, which is what it meant before the merge). From `kore-self`:

```
cargo run -p kore-self --example measure_vs_world --offline --release -- tune-v2 | tune-l | tune-b2     (and tune-v2-ext | tune-l-ext | tune-b2-ext)
cargo run -p kore-self --example measure_vs_world --offline --release -- v2 | l | b2 | rules | o 600000
cargo run -p kore-self --example measure_vs_world --offline --release -- llm out=DIR seeds=1-5 budget=60 calls=40
cargo run -p kore-self --example measure_vs_world --offline --release -- llm-report out=DIR
```

Measured 2026-11-07, branch `vs-world` from a4ca4231, Windows, 8 CPUs, a machine shared with other work (and with a nearly full disk during the LLM run, see there). Report seeds 1..20, tuning seeds 101..120. Wall time: World v2 770 s, World L 112 s, first world 31 s, rules 9 s, World O 5772 s.

## Methods compared

Every standard method is a fresh implementation in Rust (`vs_world/core.rs`, no libraries), written from the paper cited, inside the existing measurement framework: it plays exactly the worlds the being plays, through the same engines (`world2::engine`, `world_o::engine`, `lookahead::beingl`, the first world's `loop.rs`), on the same seeds and budgets, and is scored the same way (TRUE reward per energy, the checker's verdict, in World v2 and World O; the world's own reward in the first world and World L).

What a method sees (`vs_world/adapt.rs`): for every offer and each of the three fixed strategies (fast, careful, deep), the visible features the being's planner sees (family index, type, difficulty, listed reward, the strategy's listed energy; in World L also whether the offer needs a token, whether a token is held, and the previous family and result), plus the skip; after each step, the reward and energy it was TOLD (feedback noise included), read as the change of the told totals. It does not get the `discovered` strategy: that is the being's own rule book, and no method here learns rules while playing (rule learning is compared separately below). A source-scan test checks that `core.rs`, `adapt.rs`, `rules.rs` and the LLM request builder name no hidden world parameter or ground truth.

| method | source | what it does here | tuned |
|---|---|---|---|
| epsilon-greedy, decaying | Auer, Cesa-Bianchi, Fischer 2002 (eps_n-greedy) | eps_t = min(1, c K / t), K = arms seen; otherwise the best `listed reward x (s+1)/(n+2) / energy` | c, arm granularity, skip |
| UCB1-tuned | Auer, Cesa-Bianchi, Fischer 2002 | index = mean + c sqrt(ln t / n x min(1/4, V)); untried arms first | c (1 = the paper), granularity, skip |
| KL-UCB | Garivier, Cappe 2011 | largest q with n KL(mean, q) <= c ln t (bisection); untried first | c, granularity, skip |
| Thompson sampling | Thompson 1933; Agrawal, Goyal 2012 | Beta(prior + s, prior + f) per arm (fractional successes), score with the sample | prior, granularity, skip |
| EXP3 | Auer, Cesa-Bianchi, Freund, Schapire 2002 | exponential weights over the arms on offer, reward per energy scaled to [0, 1], importance-weighted update | gamma, granularity |
| LinUCB, disjoint | Li, Chu, Langford, Schapire 2010 | per strategy a ridge regression of the success share on one-hot features (bias, family, type, difficulty, type x difficulty, needs-token, token held); UCB alpha sqrt(x' A^-1 x) | alpha, skip |
| linear Thompson | Agrawal, Goyal 2013 | the same model, theta drawn from N(theta-hat, v^2 A^-1) each step (Cholesky) | v, skip |
| Q-learning | Watkins 1989; semi-Markov form Bradtke, Duff 1995; average-reward form Schwartz 1993, Das et al. 1999 | tabular Q over (visible state, family, difficulty, strategy) and the skip; target r + gamma^energy max Q', or (r - rho x energy) + max Q' with rho the told reward per energy; eps = c / sqrt(t) | c, gamma or average reward |
| SARSA | Rummery, Niranjan 1994 | as Q-learning with the next action actually taken | c, gamma or average reward |
| MLP Q-function (DQN-style) | Mnih et al. 2015 | one hidden layer of 32 tanh units on sparse features, Huber loss, SGD, replay buffer 2000, minibatch 16, target network synced every 200 steps, eps = 1 / sqrt(t) | learning rate, gamma or average reward |
| MCTS on the TRUE simulator (reference, SEES HIDDEN) | Kocsis, Szepesvari 2006 (UCT) | 200 iterations, depth 3 (World L: 4), chance nodes, expected immediate reward, opportunity-cost values with the renewal rate rho; knows the hidden success rates of the fixed strategies, the future menus and (World L) the token and phase rules and the current phase | none |

Arm granularity: (family, difficulty, strategy), like the being's cells, or (family, strategy). Skip: the being's own rule (skip when the best score is below the reward per energy told so far, after 40 energy), on or off. The value learners have the skip as one more action.

The being's own variants (not re-tuned for this track; their constants are the published ones): World v2: `learner (current)` = the B1 model and planner keyed by family with the B2 rule search every 10 steps; `recommended` = the B7 switches A, A2, B, C, G (Track B7 "all", the controller recommendation of Track B10) with the B11 settled skip test (k = 100); `all skills` = B7 + B9 experiments + B3b adaptation + rule retirement (the B10 profile without lookahead). World O: `current` and `fix+b7` (Track B11). World L: the B1 planner (current) and the B8 lookahead (pure depth 3; hybrid depth 2 = the all-skills setting). First world: the B1 model learner with B2 discovery (current), without discovery, and with the B11 skip fix.

References that SEE HIDDEN information, labelled so in every table: oracle-planner and oracle-knows-rules (World v2, World O), the oracles with fixed solvers and with perfect rules (first world), oracle one-step and oracle DP (World L), and the MCTS above. They are yardsticks, not competitors.

### Tuning (seeds 101..120 only)

Each method family's grid was run on the tuning seeds of each world (World v2: medium clean and medium hard; World L: the four settings at budget 400; first world: budgets 600 and 3000), objective fixed before the runs = the mean reward per energy over those conditions, and the family's best configuration was taken. Where the first grid chose an edge value, a second pass (`-ext`) tried values beyond it and the average-reward form of the value learners, and the better of the two passes was kept (102 + 64 configurations per world). World O uses the World v2 choice (not re-tuned: one World O life is 600,000 energy). Chosen (tuning objective in brackets):

| method | World v2 | World L | first world |
|---|---|---|---|
| eps-greedy | c 0.01, group, skip (2.887) | c 0.01, cell, no skip (1.826) | c 0.03, cell, skip (3.634) |
| UCB1-tuned | c 0.3, group, skip (2.796) | c 1, cell, no skip (1.636) | c 0.1, cell, skip (2.654) |
| KL-UCB | c 0.1, group, skip (2.809) | c 1, cell, no skip (1.641) | c 0.03, cell, skip (2.651) |
| Thompson | prior 0.5, group, skip (2.871) | prior 4, cell, no skip (1.860) | prior 0.1, cell, skip (3.660) |
| EXP3 | gamma 0.5, group (0.832) | gamma 0.3, cell (0.974) | gamma 0.5, group (1.360) |
| LinUCB | alpha 0.5, skip (3.058) | alpha 1, no skip (1.878) | alpha 0.5, skip (3.862) |
| linear Thompson | v 0.2, skip (2.932) | v 0.5, no skip (1.704) | v 0.2, skip (3.777) |
| Q-learning | eps 0.1, gamma 0.7 (1.970) | eps 0.1, gamma 0.7 (1.753) | eps 0.1, average reward (3.205) |
| SARSA | eps 0.1, average reward (1.996) | eps 0.1, gamma 0.7 (1.706) | eps 0.1, average reward (3.163) |
| MLP Q | lr 0.03, average reward (1.630) | lr 0.03, average reward (0.962) | lr 0.03, average reward (1.910) |

Some choices are still at the edge of what was tried (eps-greedy c 0.01 in World v2 and World L, Thompson prior 4 in World L, the value learners' eps 0.1): two passes were run, not more. The being's own constants were tuned on the same tuning seeds in earlier tracks, not here.

## World v2 (12 conditions, n = 20 worlds each)

True reward per energy, mean [95% interval], and paired differences (better/worse/tie of 20). The "best standard method" is the one with the highest mean on these report seeds, picked after the fact, which favours the methods.

| condition | ours current | ours recommended (B7 + skip fix) | ours all skills | best standard method (value) | recommended minus best method | current minus best method | MCTS, true simulator (sees hidden) | oracle-knows-rules (sees hidden) |
|---|---|---|---|---|---|---|---|---|
| small clean | 2.777 [2.248, 3.307] | 3.116 [2.696, 3.535] | 3.079 [2.692, 3.466] | LinUCB 3.093 [2.634, 3.552] | +0.023 [-0.256, +0.301] 13/7/0 | -0.315 [-0.566, -0.065] 8/12/0 | 3.208 [2.853, 3.563] | 3.496 [3.166, 3.827] |
| small noisy | 2.780 [2.264, 3.295] | 3.087 [2.684, 3.489] | 3.104 [2.711, 3.496] | LinUCB 3.004 [2.533, 3.475] | +0.083 [-0.223, +0.388] 12/8/0 | -0.224 [-0.466, +0.017] 9/11/0 | 3.208 [2.853, 3.563] | 3.496 [3.166, 3.827] |
| small drift | 2.724 [2.172, 3.277] | 3.004 [2.529, 3.478] | 3.037 [2.625, 3.450] | LinUCB 3.091 [2.644, 3.539] | -0.088 [-0.388, +0.213] 11/9/0 | -0.367 [-0.645, -0.089] 8/12/0 | 3.264 [2.912, 3.617] | 3.507 [3.188, 3.826] |
| small hard | 2.883 [2.346, 3.420] | 3.090 [2.645, 3.535] | 3.169 [2.749, 3.589] | eps-greedy 3.174 [2.723, 3.625] | -0.084 [-0.452, +0.285] 12/8/0 | -0.291 [-0.524, -0.058] 5/15/0 | 3.397 [3.036, 3.757] | 3.664 [3.314, 4.013] |
| medium clean | 2.755 [2.335, 3.176] | 3.193 [2.884, 3.501] | 3.187 [2.948, 3.425] | LinUCB 3.121 [2.804, 3.437] | +0.072 [-0.063, +0.207] 15/5/0 | -0.365 [-0.593, -0.138] 8/12/0 | 3.216 [2.936, 3.496] | 3.389 [3.169, 3.610] |
| medium noisy | 2.713 [2.292, 3.133] | 3.157 [2.886, 3.428] | 3.095 [2.818, 3.371] | LinUCB 3.026 [2.693, 3.360] | +0.131 [-0.073, +0.335] 15/5/0 | -0.314 [-0.565, -0.062] 6/14/0 | 3.216 [2.936, 3.496] | 3.389 [3.169, 3.610] |
| medium drift | 2.754 [2.324, 3.183] | 3.163 [2.828, 3.498] | 3.190 [2.953, 3.426] | LinUCB 3.131 [2.798, 3.464] | +0.032 [-0.166, +0.230] 14/6/0 | -0.377 [-0.607, -0.148] 5/15/0 | 3.296 [3.022, 3.571] | 3.493 [3.277, 3.709] |
| medium hard | 2.876 [2.447, 3.304] | 3.238 [2.956, 3.519] | 3.210 [2.930, 3.489] | LinUCB 3.227 [2.863, 3.591] | +0.010 [-0.248, +0.269] 12/8/0 | -0.352 [-0.628, -0.076] 4/16/0 | 3.458 [3.181, 3.735] | 3.699 [3.474, 3.924] |
| large clean | 2.987 [2.578, 3.396] | 3.221 [2.928, 3.513] | 3.162 [2.894, 3.430] | LinUCB 3.227 [2.949, 3.505] | -0.006 [-0.144, +0.132] 13/7/0 | -0.240 [-0.439, -0.042] 6/14/0 | 3.260 [3.013, 3.506] | 3.416 [3.224, 3.608] |
| large noisy | 2.967 [2.602, 3.332] | 3.224 [2.969, 3.478] | 3.145 [2.919, 3.371] | LinUCB 3.150 [2.855, 3.446] | +0.073 [-0.055, +0.202] 14/6/0 | -0.183 [-0.354, -0.012] 6/14/0 | 3.260 [3.013, 3.506] | 3.416 [3.224, 3.608] |
| large drift | 2.841 [2.437, 3.244] | 3.049 [2.744, 3.354] | 3.025 [2.763, 3.288] | LinUCB 3.096 [2.840, 3.352] | -0.047 [-0.215, +0.122] 11/9/0 | -0.255 [-0.469, -0.041] 4/16/0 | 3.236 [3.028, 3.443] | 3.363 [3.200, 3.526] |
| large hard | 2.928 [2.527, 3.328] | 3.230 [2.974, 3.487] | 3.128 [2.896, 3.360] | LinUCB 3.191 [2.906, 3.475] | +0.040 [-0.121, +0.201] 13/7/0 | -0.263 [-0.488, -0.037] 3/17/0 | 3.388 [3.176, 3.600] | 3.560 [3.383, 3.737] |

Means of every policy (intervals in the run output):

| policy | small clean | small noisy | small drift | small hard | medium clean | medium noisy | medium drift | medium hard | large clean | large noisy | large drift | large hard |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ours: learner (current) | 2.777 | 2.780 | 2.724 | 2.883 | 2.755 | 2.713 | 2.754 | 2.876 | 2.987 | 2.967 | 2.841 | 2.928 |
| ours: B7 + skip fix (recommended) | 3.116 | 3.087 | 3.004 | 3.090 | 3.193 | 3.157 | 3.163 | 3.238 | 3.221 | 3.224 | 3.049 | 3.230 |
| ours: all skills (B7+B9+B3b+retire) | 3.079 | 3.104 | 3.037 | 3.169 | 3.187 | 3.095 | 3.190 | 3.210 | 3.162 | 3.145 | 3.025 | 3.128 |
| eps-greedy | 2.945 | 2.894 | 2.929 | 3.174 | 2.922 | 2.929 | 2.946 | 3.224 | 2.941 | 2.879 | 2.752 | 3.023 |
| UCB1-tuned | 2.852 | 2.670 | 2.811 | 2.798 | 2.703 | 2.563 | 2.729 | 2.758 | 2.860 | 2.744 | 2.713 | 2.816 |
| KL-UCB | 2.856 | 2.745 | 2.813 | 2.902 | 2.756 | 2.645 | 2.773 | 2.792 | 2.880 | 2.784 | 2.729 | 2.847 |
| Thompson | 2.915 | 2.835 | 2.904 | 3.088 | 2.963 | 2.840 | 2.968 | 3.091 | 2.979 | 2.882 | 2.823 | 2.949 |
| EXP3 | 0.933 | 0.903 | 0.952 | 0.964 | 0.894 | 0.874 | 0.906 | 0.932 | 0.911 | 0.886 | 0.873 | 0.896 |
| LinUCB | 3.093 | 3.004 | 3.091 | 3.111 | 3.121 | 3.026 | 3.131 | 3.227 | 3.227 | 3.150 | 3.096 | 3.191 |
| linear Thompson | 3.065 | 2.983 | 3.048 | 3.148 | 3.051 | 2.967 | 3.059 | 3.179 | 3.135 | 3.042 | 2.992 | 3.117 |
| Q-learning | 2.111 | 1.971 | 2.114 | 2.136 | 2.080 | 1.986 | 2.090 | 2.072 | 2.154 | 2.105 | 2.095 | 2.158 |
| SARSA | 2.084 | 1.993 | 2.094 | 2.098 | 2.088 | 1.945 | 2.015 | 2.072 | 2.174 | 2.130 | 2.072 | 2.147 |
| MLP Q (DQN-style) | 1.441 | 1.435 | 1.442 | 1.490 | 1.770 | 1.679 | 1.757 | 1.793 | 2.002 | 1.894 | 1.877 | 1.923 |
| MCTS on the TRUE simulator (sees hidden) | 3.208 | 3.208 | 3.264 | 3.397 | 3.216 | 3.216 | 3.296 | 3.458 | 3.260 | 3.260 | 3.236 | 3.388 |
| oracle-planner (sees hidden) | 3.324 | 3.324 | 3.350 | 3.528 | 3.232 | 3.232 | 3.335 | 3.547 | 3.265 | 3.265 | 3.217 | 3.433 |
| oracle-knows-rules (sees hidden) | 3.496 | 3.496 | 3.507 | 3.664 | 3.389 | 3.389 | 3.493 | 3.699 | 3.416 | 3.416 | 3.363 | 3.560 |

Ours (recommended) minus each method, paired (better/worse/tie):

| method | small clean | small noisy | small drift | small hard | medium clean | medium noisy | medium drift | medium hard | large clean | large noisy | large drift | large hard |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| eps-greedy | +0.170 [-0.098, +0.439] 15/5/0 | +0.193 [-0.097, +0.483] 16/4/0 | +0.075 [-0.236, +0.387] 15/5/0 | -0.084 [-0.452, +0.285] 12/8/0 | +0.271 [+0.110, +0.432] 16/4/0 | +0.229 [+0.006, +0.451] 18/2/0 | +0.217 [+0.001, +0.433] 17/3/0 | +0.013 [-0.246, +0.273] 15/5/0 | +0.280 [+0.124, +0.435] 17/3/0 | +0.345 [+0.219, +0.471] 18/2/0 | +0.297 [+0.122, +0.473] 16/4/0 | +0.207 [+0.053, +0.362] 17/3/0 |
| UCB1-tuned | +0.264 [+0.011, +0.516] 15/5/0 | +0.416 [+0.109, +0.724] 17/3/0 | +0.192 [-0.093, +0.478] 14/6/0 | +0.292 [-0.084, +0.669] 16/4/0 | +0.490 [+0.273, +0.706] 18/2/0 | +0.594 [+0.442, +0.746] 19/1/0 | +0.434 [+0.182, +0.686] 18/2/0 | +0.480 [+0.296, +0.664] 19/1/0 | +0.361 [+0.239, +0.483] 18/2/0 | +0.479 [+0.340, +0.618] 19/1/0 | +0.336 [+0.171, +0.501] 17/3/0 | +0.415 [+0.253, +0.576] 19/1/0 |
| KL-UCB | +0.259 [+0.011, +0.507] 15/5/0 | +0.341 [+0.075, +0.608] 17/3/0 | +0.191 [-0.092, +0.474] 14/6/0 | +0.188 [-0.130, +0.507] 16/4/0 | +0.437 [+0.225, +0.648] 18/2/0 | +0.512 [+0.396, +0.628] 19/1/0 | +0.390 [+0.143, +0.637] 18/2/0 | +0.446 [+0.231, +0.661] 19/1/0 | +0.341 [+0.213, +0.468] 18/2/0 | +0.439 [+0.303, +0.576] 19/1/0 | +0.320 [+0.151, +0.490] 17/3/0 | +0.383 [+0.205, +0.562] 19/1/0 |
| Thompson | +0.201 [-0.054, +0.456] 15/5/0 | +0.251 [-0.016, +0.518] 16/4/0 | +0.100 [-0.199, +0.399] 14/6/0 | +0.002 [-0.350, +0.354] 13/7/0 | +0.230 [+0.100, +0.360] 18/2/0 | +0.317 [+0.098, +0.537] 18/2/0 | +0.195 [-0.010, +0.400] 17/2/1 | +0.147 [-0.126, +0.419] 17/3/0 | +0.242 [+0.104, +0.380] 16/4/0 | +0.342 [+0.235, +0.448] 19/1/0 | +0.227 [+0.044, +0.409] 16/4/0 | +0.281 [+0.125, +0.437] 19/1/0 |
| EXP3 | +2.182 [+1.809, +2.555] 20/0/0 | +2.183 [+1.877, +2.490] 20/0/0 | +2.052 [+1.626, +2.478] 20/0/0 | +2.126 [+1.746, +2.506] 20/0/0 | +2.298 [+2.052, +2.545] 20/0/0 | +2.283 [+2.070, +2.496] 20/0/0 | +2.256 [+1.967, +2.545] 20/0/0 | +2.306 [+2.065, +2.546] 20/0/0 | +2.310 [+2.056, +2.565] 20/0/0 | +2.337 [+2.122, +2.553] 20/0/0 | +2.177 [+1.914, +2.440] 20/0/0 | +2.335 [+2.127, +2.542] 20/0/0 |
| LinUCB | +0.023 [-0.256, +0.301] 13/7/0 | +0.083 [-0.223, +0.388] 12/8/0 | -0.088 [-0.388, +0.213] 11/9/0 | -0.020 [-0.380, +0.339] 10/10/0 | +0.072 [-0.063, +0.207] 15/5/0 | +0.131 [-0.073, +0.335] 15/5/0 | +0.032 [-0.166, +0.230] 14/6/0 | +0.010 [-0.248, +0.269] 12/8/0 | -0.006 [-0.144, +0.132] 13/7/0 | +0.073 [-0.055, +0.202] 14/6/0 | -0.047 [-0.215, +0.122] 11/9/0 | +0.040 [-0.121, +0.201] 13/7/0 |
| linear Thompson | +0.051 [-0.229, +0.331] 13/7/0 | +0.104 [-0.185, +0.393] 13/7/0 | -0.045 [-0.340, +0.250] 12/8/0 | -0.057 [-0.408, +0.293] 12/8/0 | +0.142 [-0.000, +0.285] 16/4/0 | +0.191 [-0.013, +0.394] 17/3/0 | +0.104 [-0.095, +0.302] 16/4/0 | +0.059 [-0.197, +0.315] 15/5/0 | +0.086 [-0.032, +0.204] 16/4/0 | +0.182 [+0.071, +0.293] 18/2/0 | +0.058 [-0.104, +0.219] 16/4/0 | +0.114 [-0.037, +0.264] 18/2/0 |
| Q-learning | +1.004 [+0.664, +1.345] 18/2/0 | +1.115 [+0.774, +1.457] 18/2/0 | +0.890 [+0.528, +1.251] 16/4/0 | +0.954 [+0.550, +1.358] 16/4/0 | +1.112 [+0.923, +1.301] 20/0/0 | +1.172 [+0.955, +1.388] 19/1/0 | +1.073 [+0.831, +1.314] 18/2/0 | +1.166 [+0.913, +1.419] 19/1/0 | +1.067 [+0.855, +1.280] 20/0/0 | +1.118 [+0.942, +1.294] 20/0/0 | +0.955 [+0.726, +1.184] 20/0/0 | +1.073 [+0.880, +1.266] 20/0/0 |
| SARSA | +1.032 [+0.706, +1.357] 18/2/0 | +1.094 [+0.751, +1.437] 18/2/0 | +0.910 [+0.572, +1.248] 16/4/0 | +0.993 [+0.624, +1.362] 18/2/0 | +1.104 [+0.903, +1.306] 19/1/0 | +1.212 [+1.024, +1.400] 20/0/0 | +1.148 [+0.900, +1.395] 18/2/0 | +1.165 [+0.953, +1.377] 20/0/0 | +1.047 [+0.816, +1.278] 19/1/0 | +1.094 [+0.927, +1.260] 20/0/0 | +0.977 [+0.745, +1.209] 19/1/0 | +1.083 [+0.898, +1.269] 19/1/0 |
| MLP Q (DQN-style) | +1.675 [+1.271, +2.078] 20/0/0 | +1.652 [+1.355, +1.949] 20/0/0 | +1.562 [+1.129, +1.995] 20/0/0 | +1.600 [+1.242, +1.958] 19/1/0 | +1.423 [+1.187, +1.659] 20/0/0 | +1.478 [+1.243, +1.712] 20/0/0 | +1.406 [+1.108, +1.704] 19/1/0 | +1.445 [+1.180, +1.709] 20/0/0 | +1.219 [+0.967, +1.470] 20/0/0 | +1.329 [+1.125, +1.534] 20/0/0 | +1.172 [+0.892, +1.452] 19/1/0 | +1.308 [+1.097, +1.518] 20/0/0 |

The current learner, the recommended profile and oracle-knows-rules reproduce the published World v2 / B7 / B11 numbers (current 2.777 ... 2.928 as in "World v2"; recommended within 0.01 of B7 "all", which it equals apart from the skip test).

Reading:
* **The current learner (B1 planner + B2 search) loses to the best standard method.** LinUCB (the best method in 11 of 12 conditions; eps-greedy in small hard) is above it by 0.18 to 0.38, the interval excluding 0 in 11 of 12 conditions (not small noisy, -0.224 [-0.466, +0.017]), worse in 11 to 17 of 20 worlds. Tuned linear Thompson, Thompson and eps-greedy are also above it on the mean in most conditions.
* **The recommended profile (B7 rule use + B11 skip fix) ties the best method:** recommended minus best method -0.088 to +0.131, no interval excludes 0 (better in 11 to 15 of 20 worlds). It is above UCB1-tuned and KL-UCB with the interval above 0 in 10 of 12 conditions, above eps-greedy in 7 and Thompson in 6, above linear Thompson only in large noisy, and above EXP3, Q-learning, SARSA and the MLP in all 12 (by 0.9 to 2.3).
* So in World v2 the being's choosing part is weaker than a tuned linear contextual bandit on the same visible features (LinUCB shares evidence across families and difficulties through its type x difficulty features; the B1 model shares only along its family hierarchy and keeps a large optimism bonus), and the B7 rule use (fitted constants, no redundant `discovered` arm, rule-record estimates) makes up the difference. LinUCB uses no rules. A planner that combines LinUCB-style sharing with the B7 rule use was not built or measured.
* The all-skills profile is within 0.1 of the recommended one, as in Track B10.
* References that see hidden information: MCTS on the true simulator 3.21 to 3.46, oracle-planner 3.22 to 3.55, oracle-knows-rules 3.36 to 3.70; the recommended profile is 0.19 to 0.57 below oracle-knows-rules. The MCTS reference is below the oracle-planner in most conditions (200 iterations, depth 3): it is an upper bound in what it knows, not in what it achieves.
* The value learners are far below the bandits here: the menus are independent from step to step, so their state carries nothing, and their exploration is undirected. The average-reward form helped them on the tuning seeds (Q 1.74 -> 1.97, MLP 1.46 -> 1.63) but not enough. EXP3, made for adversarial rewards, explores almost uniformly over hundreds of arms and is the worst method everywhere.

## First world with the B2 pattern families (extended pattern world, pattern reward x2; n = 20)

| policy | budget 600 | budget 3000 |
|---|---|---|
| ours: learner + discovery (current, B2) | 3.142 [2.773, 3.512] | 3.943 [3.583, 4.303] |
| ours: learner, no discovery | 3.178 [2.801, 3.556] | 3.781 [3.412, 4.150] |
| ours: learner + discovery + skip fix | 3.142 [2.774, 3.510] | 3.975 [3.603, 4.347] |
| eps-greedy (c 0.03, cell, skip) | 3.440 [3.117, 3.764] | 3.651 [3.362, 3.941] |
| UCB1-tuned (c 0.1, cell, skip) | 1.629 [1.435, 1.823] | 3.525 [3.222, 3.827] |
| KL-UCB (c 0.03, cell, skip) | 1.640 [1.446, 1.834] | 3.529 [3.221, 3.836] |
| Thompson (prior 0.1, cell, skip) | 3.343 [2.968, 3.717] | 3.793 [3.461, 4.125] |
| EXP3 (gamma 0.5, group) | 1.296 [1.173, 1.418] | 1.433 [1.305, 1.562] |
| LinUCB (alpha 0.5, skip) | 3.592 [3.226, 3.958] | 3.936 [3.623, 4.249] |
| linear Thompson (v 0.2, skip) | 3.379 [3.043, 3.715] | 3.815 [3.521, 4.109] |
| Q-learning (eps 0.1, average reward) | 2.961 [2.664, 3.259] | 3.474 [3.160, 3.787] |
| SARSA (eps 0.1, average reward) | 2.852 [2.571, 3.133] | 3.421 [3.124, 3.718] |
| MLP Q (lr 0.03, average reward) | 1.367 [1.064, 1.671] | 2.448 [2.071, 2.825] |
| oracle, fixed solvers (sees hidden) | 3.862 [3.515, 4.210] | 3.911 [3.590, 4.233] |
| oracle with perfect rules (sees hidden) | 4.270 [3.901, 4.639] | 4.272 [3.920, 4.624] |

| current minus method (better/worse/tie) | budget 600 | budget 3000 |
|---|---|---|
| LinUCB | -0.450 [-0.659, -0.241] 1/19/0 | +0.007 [-0.105, +0.119] 8/12/0 |
| eps-greedy | -0.298 [-0.497, -0.099] 5/15/0 | +0.292 [+0.165, +0.418] 18/2/0 |
| linear Thompson | -0.237 [-0.480, +0.006] 8/12/0 | +0.128 [-0.016, +0.273] 14/6/0 |
| Thompson | -0.200 [-0.396, -0.004] 5/15/0 | +0.150 [+0.042, +0.258] 18/2/0 |
| Q-learning (average reward) | +0.181 [-0.169, +0.531] 12/8/0 | +0.470 [+0.268, +0.672] 17/3/0 |
| UCB1-tuned | +1.513 [+1.213, +1.813] 20/0/0 | +0.419 [+0.286, +0.551] 20/0/0 |
| (skip fix minus current) | -0.000 [-0.008, +0.007] 3/4/13 | +0.032 [+0.003, +0.061] 13/6/1 |

Reading: **at the short budget (600 energy, about 160 attempts) the being loses to four standard methods**: LinUCB by 0.45 [0.24, 0.66] (worse in 19 of 20 worlds), eps-greedy by 0.30, Thompson by 0.20, linear Thompson by 0.24 (interval just includes 0). At budget 3000 it ties LinUCB (+0.007 [-0.105, +0.119]) and is above every other method (eps-greedy +0.29, Thompson +0.15; linear Thompson +0.13, interval includes 0). The cost at 600 is the B1 learner's early exploration (optimism 1.5 x sd on every (domain, difficulty, strategy) cell; Track B1's own learning curve rises from 2.53 to 3.27 over its first windows). The published numbers reproduce exactly (3.142, 3.178, 3.943, 3.781, 3.862, 3.911, 4.270, 4.272).

## World L (four settings x two budgets, n = 20)

| setting, budget | ours B1 (current) | best of ours (which) | best standard method (value) | B1 minus best method | best of ours minus best method | MCTS, true simulator (sees hidden) | oracle DP (sees hidden) |
|---|---|---|---|---|---|---|---|
| plain 400 | 2.060 [1.855, 2.265] | 2.060 (B1) | eps-greedy 2.130 [1.924, 2.337] | -0.071 [-0.206, +0.065] 12/8/0 | -0.071 [-0.206, +0.065] 12/8/0 | 2.184 [2.000, 2.368] | 2.242 [2.061, 2.422] |
| keys 400 | 1.105 [0.932, 1.277] | 1.329 (pure d3) | LinUCB 1.228 [1.122, 1.333] | -0.123 [-0.256, +0.010] 3/17/0 | +0.102 [-0.061, +0.265] 12/8/0 | 1.855 [1.732, 1.979] | 1.972 [1.856, 2.088] |
| probe 400 | 2.147 [1.877, 2.418] | 2.150 (pure d3) | eps-greedy 2.359 [2.172, 2.545] | -0.211 [-0.380, -0.043] 0/9/11 | -0.209 [-0.373, -0.045] 3/11/6 | 2.865 [2.680, 3.050] | 2.718 [2.537, 2.899] |
| sizes 400 | 1.922 [1.778, 2.066] | 1.973 (hybrid d2) | LinUCB 2.063 [1.940, 2.187] | -0.142 [-0.274, -0.009] 8/12/0 | -0.090 [-0.203, +0.023] 8/12/0 | 2.088 [1.989, 2.188] | 2.166 [2.048, 2.284] |
| plain 1600 | 2.073 [1.877, 2.268] | 2.112 (pure d3) | LinUCB 2.163 [1.966, 2.360] | -0.090 [-0.219, +0.039] 6/14/0 | -0.050 [-0.091, -0.010] 5/15/0 | 2.175 [1.996, 2.354] | 2.224 [2.034, 2.415] |
| keys 1600 | 1.108 [0.935, 1.281] | 1.575 (pure d3) | Thompson 1.231 [1.128, 1.333] | -0.122 [-0.254, +0.009] 3/17/0 | +0.344 [+0.196, +0.493] 17/3/0 | 1.900 [1.797, 2.004] | 2.002 [1.892, 2.112] |
| probe 1600 | 2.214 [1.987, 2.440] | 2.214 (B1) | eps-greedy 2.361 [2.231, 2.492] | -0.148 [-0.317, +0.022] 1/11/8 | -0.148 [-0.317, +0.022] 1/11/8 | 2.889 [2.737, 3.041] | 2.724 [2.574, 2.873] |
| sizes 1600 | 1.975 [1.819, 2.131] | 2.043 (hybrid d2) | LinUCB 2.106 [1.986, 2.226] | -0.131 [-0.227, -0.035] 4/16/0 | -0.063 [-0.103, -0.024] 4/16/0 | 2.103 [2.003, 2.203] | 2.186 [2.074, 2.299] |

("Best of ours" is also picked after the fact, among the B1 planner and the two lookahead settings.)

Means of every policy:

| policy | plain 400 | keys 400 | probe 400 | sizes 400 | plain 1600 | keys 1600 | probe 1600 | sizes 1600 |
|---|---|---|---|---|---|---|---|---|
| ours: B1 planner (current) | 2.060 | 1.105 | 2.147 | 1.922 | 2.073 | 1.108 | 2.214 | 1.975 |
| ours: lookahead pure d3 | 2.041 | 1.329 | 2.150 | 1.957 | 2.112 | 1.575 | 2.203 | 2.014 |
| ours: lookahead hybrid d2 (all-skills setting) | 2.054 | 1.127 | 2.098 | 1.973 | 2.065 | 1.126 | 2.028 | 2.043 |
| eps-greedy | 2.130 | 1.218 | 2.359 | 2.003 | 2.151 | 1.220 | 2.361 | 2.059 |
| UCB1-tuned | 1.912 | 1.162 | 2.009 | 1.596 | 2.112 | 1.204 | 2.271 | 1.989 |
| KL-UCB | 1.910 | 1.171 | 2.102 | 1.596 | 2.113 | 1.219 | 2.295 | 1.966 |
| Thompson | 2.078 | 1.204 | 2.313 | 1.940 | 2.151 | 1.231 | 2.330 | 2.022 |
| EXP3 | 1.071 | 0.814 | 1.071 | 1.068 | 1.226 | 0.822 | 1.200 | 1.092 |
| LinUCB | 2.091 | 1.228 | 2.355 | 2.063 | 2.163 | 1.227 | 2.359 | 2.106 |
| linear Thompson | 1.992 | 1.138 | 2.021 | 1.858 | 2.089 | 1.201 | 2.233 | 1.995 |
| Q-learning | 1.826 | 1.169 | 2.231 | 1.722 | 1.904 | 1.201 | 2.256 | 1.803 |
| SARSA | 1.768 | 1.135 | 2.196 | 1.714 | 1.849 | 1.163 | 2.117 | 1.751 |
| MLP Q (DQN-style) | 1.175 | 0.813 | 1.015 | 1.137 | 1.538 | 1.073 | 1.259 | 1.399 |
| MCTS on the TRUE simulator (sees hidden, incl. the phase) | 2.184 | 1.855 | 2.865 | 2.088 | 2.175 | 1.900 | 2.889 | 2.103 |
| oracle one-step (sees hidden) | 2.209 | 1.230 | 2.379 | 2.120 | 2.200 | 1.225 | 2.375 | 2.141 |
| oracle DP (sees hidden) | 2.242 | 1.972 | 2.718 | 2.166 | 2.224 | 2.002 | 2.724 | 2.186 |

Reading:
* **The B1 planner (current) is below the best standard method in all 8 setting-budget pairs** (-0.07 to -0.21; interval below 0 in probe 400, sizes 400 and sizes 1600). In `plain`, `probe` and `sizes`, eps-greedy, LinUCB and Thompson (no skipping, finer arms) do a little better. The B1 and lookahead numbers reproduce Track B8.
* **The B8 lookahead (pure, depth 3) beats every standard method in `keys`** (the unlock setting): +0.34 [+0.20, +0.49] over the best method at budget 1600 (better in 17 of 20 worlds), +0.10 [-0.06, +0.27] at 400. None of the methods learns the key -> vault chain: tabular Q-learning and SARSA have the token in their state but stay at 1.14 to 1.20, like the bandits (1.14 to 1.23); the MLP 0.81 to 1.07.
* Elsewhere the best of ours is below the best method: probe 400 -0.21 [-0.37, -0.05], plain 1600 -0.050 [-0.091, -0.010], sizes 1600 -0.063 [-0.103, -0.024]; the others include 0.
* The references stay far above in `keys` and `probe`: oracle DP 1.97 to 2.00 (keys) and 2.72 (probe); MCTS on the true simulator, which also knows the current hidden phase, 2.87 to 2.89 in `probe` (above the DP oracle, which does not know the phase) and 1.86 to 1.90 in `keys`.

## World O (open-ended), budget 600,000 energy units, windows of 100,000, n = 20

Standard methods use their World v2 tuning (not re-tuned for World O). The being's variants are those of Track B11 (`current`, and `fix+b7` = the settled skip test with B7 A and A2, the closest World O has to the recommended profile; B3b and the B7 constants are not available there). Regret = oracle-knows-rules minus the policy, per window of 100,000 energy units, paired by world; "whole run" is the reward per energy after the first window.

| policy | whole run (after window 1) | regret 0k-100k | regret 100k-200k | regret 200k-300k | regret 300k-400k | regret 400k-500k | regret 500k-600k |
|---|---|---|---|---|---|---|---|
| ours: current | 4.191 [4.017, 4.365] | 0.131 [0.044, 0.218] | 0.239 [0.075, 0.403] | 0.537 [0.114, 0.961] | 0.549 [0.242, 0.857] | 0.460 [0.241, 0.678] | 0.805 [0.377, 1.233] |
| ours: fix+b7 (recommended) | 3.980 [3.787, 4.174] | 0.115 [0.023, 0.206] | 0.316 [0.115, 0.518] | 0.506 [0.236, 0.776] | 0.936 [0.564, 1.309] | 0.889 [0.569, 1.208] | 0.998 [0.559, 1.437] |
| oracle-planner (sees hidden) | 3.863 [3.628, 4.098] | 0.005 [-0.004, 0.013] | 0.300 [0.129, 0.471] | 0.703 [0.269, 1.137] | 1.280 [0.739, 1.820] | 1.021 [0.611, 1.431] | 0.927 [0.505, 1.349] |
| oracle-knows-rules (sees hidden) | 4.709 [4.633, 4.785] | 0.000 [0.000, 0.000] | 0.000 [0.000, 0.000] | 0.000 [0.000, 0.000] | 0.000 [0.000, 0.000] | 0.000 [0.000, 0.000] | 0.000 [0.000, 0.000] |
| eps-greedy | 3.227 [2.977, 3.476] | 0.600 [0.503, 0.697] | 0.678 [0.411, 0.945] | 1.294 [0.682, 1.905] | 1.834 [1.232, 2.436] | 1.952 [1.435, 2.469] | 1.655 [1.069, 2.240] |
| UCB1-tuned | 3.485 [3.247, 3.723] | 0.282 [0.219, 0.346] | 0.541 [0.333, 0.750] | 1.074 [0.623, 1.526] | 1.700 [1.114, 2.286] | 1.487 [1.032, 1.942] | 1.318 [0.907, 1.729] |
| KL-UCB | 3.544 [3.303, 3.786] | 0.262 [0.216, 0.309] | 0.491 [0.300, 0.683] | 1.002 [0.532, 1.471] | 1.652 [1.093, 2.210] | 1.457 [1.027, 1.887] | 1.223 [0.774, 1.672] |
| Thompson (Beta) | 3.651 [3.419, 3.883] | 0.243 [0.205, 0.280] | 0.444 [0.258, 0.630] | 0.888 [0.417, 1.359] | 1.529 [0.978, 2.079] | 1.326 [0.927, 1.724] | 1.106 [0.686, 1.527] |
| EXP3 | 1.522 [1.459, 1.585] | 1.430 [1.346, 1.514] | 2.440 [2.340, 2.540] | 3.229 [3.100, 3.357] | 3.425 [3.270, 3.580] | 3.411 [3.255, 3.567] | 3.431 [3.310, 3.552] |
| LinUCB | 3.573 [3.315, 3.832] | 0.134 [0.094, 0.174] | 0.396 [0.210, 0.582] | 0.908 [0.448, 1.367] | 1.655 [1.049, 2.260] | 1.402 [0.963, 1.840] | 1.319 [0.842, 1.796] |
| linear Thompson | 3.590 [3.338, 3.841] | 0.156 [0.119, 0.193] | 0.377 [0.194, 0.561] | 0.862 [0.404, 1.320] | 1.710 [1.143, 2.277] | 1.386 [0.958, 1.813] | 1.262 [0.791, 1.733] |
| Q-learning | 1.095 [0.971, 1.218] | 1.135 [0.997, 1.274] | 2.709 [2.507, 2.912] | 3.617 [3.272, 3.961] | 3.811 [3.545, 4.078] | 3.841 [3.613, 4.068] | 4.095 [3.787, 4.402] |
| SARSA | 2.461 [2.354, 2.568] | 0.379 [0.301, 0.458] | 1.284 [1.099, 1.468] | 2.174 [1.969, 2.378] | 2.457 [2.224, 2.690] | 2.712 [2.436, 2.988] | 2.616 [2.456, 2.776] |
| MLP Q (DQN-style) | 3.594 [3.316, 3.871] | 0.148 [0.134, 0.162] | 0.473 [0.288, 0.659] | 1.039 [0.551, 1.526] | 1.607 [1.036, 2.178] | 1.259 [0.837, 1.682] | 1.198 [0.771, 1.625] |

| policy minus ours (recommended), whole run after window 1 | difference (better/worse/tie) | minus ours (current) |
|---|---|---|
| oracle-planner (sees hidden) | -0.117 [-0.285, +0.051] 9/11/0 | -0.328 [-0.495, -0.161] 4/16/0 |
| oracle-knows-rules (sees hidden) | +0.729 [+0.564, +0.895] 20/0/0 | +0.518 [+0.378, +0.658] 20/0/0 |
| eps-greedy | -0.753 [-0.952, -0.555] 0/20/0 | -0.964 [-1.159, -0.770] 0/20/0 |
| UCB1-tuned | -0.495 [-0.657, -0.333] 1/19/0 | -0.706 [-0.883, -0.529] 0/20/0 |
| KL-UCB | -0.436 [-0.606, -0.265] 1/19/0 | -0.647 [-0.826, -0.468] 1/19/0 |
| Thompson (Beta) | -0.329 [-0.483, -0.176] 1/19/0 | -0.540 [-0.701, -0.380] 0/20/0 |
| EXP3 | -2.458 [-2.624, -2.293] 0/20/0 | -2.669 [-2.814, -2.525] 0/20/0 |
| LinUCB | -0.407 [-0.588, -0.226] 2/18/0 | -0.618 [-0.819, -0.417] 1/19/0 |
| linear Thompson | -0.390 [-0.558, -0.223] 1/19/0 | -0.601 [-0.785, -0.418] 2/18/0 |
| Q-learning | -2.886 [-3.072, -2.699] 0/20/0 | -3.097 [-3.243, -2.950] 0/20/0 |
| SARSA | -1.519 [-1.703, -1.336] 0/20/0 | -1.730 [-1.889, -1.572] 0/20/0 |
| MLP Q (DQN-style) | -0.386 [-0.584, -0.188] 2/18/0 | -0.597 [-0.786, -0.409] 0/20/0 |

Order versus competence (top tier, Spearman of family index with attempts to competence, per world; negative = faster later) and share of learnable families reached

| policy | worlds | Spearman mean [95% interval] | learnable families reached competence |
|---|---|---|---|
| ours: current | 20 | -0.053 [-0.156, 0.050] | 803 of 1137 (71%) |
| ours: fix+b7 (recommended) | 20 | -0.032 [-0.136, 0.071] | 560 of 902 (62%) |
| eps-greedy | 7 | -0.117 [-0.409, 0.174] | 286 of 303 (94%) |
| UCB1-tuned | 20 | -0.134 [-0.295, 0.027] | 432 of 530 (82%) |
| KL-UCB | 20 | -0.146 [-0.292, -0.001] | 446 of 541 (82%) |
| Thompson (Beta) | 20 | -0.043 [-0.117, 0.030] | 496 of 1475 (34%) |
| EXP3 | 20 | 0.047 [-0.012, 0.106] | 636 of 1766 (36%) |
| LinUCB | 20 | -0.129 [-0.280, 0.022] | 430 of 566 (76%) |
| linear Thompson | 20 | -0.123 [-0.286, 0.040] | 441 of 634 (70%) |
| Q-learning | 20 | 0.058 [-0.058, 0.174] | 272 of 805 (34%) |
| SARSA | 20 | 0.030 [-0.057, 0.116] | 527 of 1536 (34%) |
| MLP Q (DQN-style) | 20 | -0.051 [-0.130, 0.028] | 553 of 1556 (36%) |

Reading:
* **World O is where the being wins clearly.** The current learner (4.191 [4.017, 4.365] after the first window) is above every standard method by 0.54 to 3.10, better in 18 to 20 of 20 worlds against each (best method: Thompson 3.651, current minus Thompson +0.540 [+0.380, +0.701] 20/0/0; LinUCB +0.618, linear Thompson +0.601, the MLP +0.597). It is even above the oracle-planner (+0.328 [+0.161, +0.495], which knows the fixed strategies' success rates but has no rules). The difference is rule knowledge and the being's handling of the open world: World O keeps opening families whose hidden rule the B2 search can learn, and the methods have no rules (and the linear models and the MLP cannot forget a closed slot).
* The B11 variant `fix+b7` is again below `current` (-0.211, as in Track B11, where b7 cost -0.244) but still above every standard method (+0.33 to +2.89, better in 18 to 20 of 20 worlds).
* The regret of every policy grows as the families reach the top tier (as Track B11 found for the being; its window values here reproduce B11's first windows: 0.131, 0.239, 0.537, 0.549): the methods' regret is 1.1 to 4.1 in the last window against 0.81 for `current`.
* Order versus competence: no policy gets clearly faster on later top-tier families (Spearman -0.15 to +0.06; only KL-UCB's interval just excludes 0, -0.146 [-0.292, -0.001]). The share of learnable families that reach competence is not comparable across policies: it counts only families a policy attempted, and most bandits attempt far fewer (eps-greedy 303, UCB1-tuned 530, KL-UCB 541, LinUCB 566, linear Thompson 634 learnable families against 1137 for `current`), the ones they keep returning to.
* Wall time 5772 s (280 lives of 600,000 energy, shared machine).

## Rule learning on revealed examples (World v2, 12 families, seeds 1..20)

For each sequence signature (family, difficulty 1..5) a learner gets k revealed examples (prefix and revealed next term; with noise 0.1 the revealed term is the checker's +-1..3 with probability 0.1, the World v2 noise model) and answers 50 fresh tasks of that signature. The same examples for every learner. Shares of fresh tasks, mean over 20 worlds (95% intervals in the run output; all intervals of the "right" column are within +-0.09 of the mean). "In-grammar" = the signature's hidden class is expressible in the 17-form DSL (measurement only).

| noise | k | learner | right | wrong | abstain | right on in-grammar signatures | wrong on outside-grammar signatures |
|---|---|---|---|---|---|---|---|
| 0 | 4 | ours: B2 search (exact rational fit + guard) | 0.784 | 0.000 | 0.216 | 0.966 | 0.000 |
| 0 | 4 | brute-force enumeration (same DSL and guard, integer grid) | 0.590 | 0.000 | 0.410 | 0.731 | 0.000 |
| 0 | 4 | 1-NN on prefix features | 0.147 | 0.853 | 0 | 0.148 | 0.519 |
| 0 | 4 | regression tree on prefix features | 0.145 | 0.855 | 0 | 0.148 | 0.521 |
| 0 | 32 | ours: B2 search | 0.809 | 0.000 | 0.191 | 0.996 | 0.000 |
| 0 | 32 | brute-force enumeration | 0.707 | 0.000 | 0.293 | 0.875 | 0.000 |
| 0 | 32 | 1-NN | 0.485 | 0.515 | 0 | 0.540 | 0.443 |
| 0 | 32 | 3-NN | 0.352 | 0.648 | 0 | 0.403 | 0.511 |
| 0 | 32 | regression tree | 0.547 | 0.453 | 0 | 0.629 | 0.483 |
| 0.1 | 4 | ours: B2 search (= B7 robust guard here) | 0.550 | 0.000 | 0.450 | 0.676 | 0.000 |
| 0.1 | 8 | ours: B2 search | 0.353 | 0.000 | 0.647 | 0.432 | 0.000 |
| 0.1 | 8 | ours: B7 robust guard (theta 0.4) | 0.773 | 0.000 | 0.227 | 0.952 | 0.000 |
| 0.1 | 32 | ours: B2 search | 0.029 | 0.000 | 0.971 | 0.034 | 0.000 |
| 0.1 | 32 | ours: B7 robust guard | 0.809 | 0.000 | 0.191 | 0.996 | 0.000 |
| 0.1 | 32 | brute-force enumeration | 0.058 | 0.000 | 0.942 | 0.073 | 0.000 |
| 0.1 | 32 | 1-NN | 0.441 | 0.559 | 0 | 0.490 | 0.454 |
| 0.1 | 32 | regression tree | 0.477 | 0.523 | 0 | 0.549 | 0.495 |

Formula families (hidden constant g), k revealed examples per family, right share on 50 fresh tasks:

| noise | k | ours: fitted constant, exact guard (B7 C) | ours: fitted constant, robust guard | 1-NN on the input | regression tree on (kind, input) |
|---|---|---|---|---|---|
| 0 | 2 | 0.000 (abstains: needs 3 + 1 examples) | 0.000 | 0.136 | 0.136 |
| 0 | 4 | 1.000 | 1.000 | 0.241 | 0.241 |
| 0 | 16 | 1.000 | 1.000 | 0.616 | 0.616 |
| 0.1 | 4 | 0.775 | 0.775 | 0.226 | 0.224 |
| 0.1 | 8 | 0.500 (abstains 0.500) | 0.975 | 0.383 | 0.379 |
| 0.1 | 16 | 0.100 (abstains 0.900) | 1.000 | 0.540 | 0.524 |

The verified learners never answered wrong in these runs; the nearest-neighbour and tree predictors always answer, so whatever they do not get right is wrong.

Reading: **on rule learning the being's verified search is clearly ahead of the standard alternatives tried here.** From 4 clean examples it answers 78% of fresh tasks right and none wrong (97% on the signatures its DSL can express); the same DSL and guard with brute-force integer enumeration gets 59% (it misses rational and larger coefficients); k-NN and regression trees on prefix features get 5% to 15% right from 4 examples and at most 55% from 32, and are wrong on everything else. Weak point confirmed: under 10% feedback noise the exact guard (and brute force) gets worse with more examples (B2 0.55 -> 0.03 from 4 to 32 examples), because one corrupted example vetoes a correct rule for good; the B7 robust guard removes that (0.77 to 0.81 right, 0 wrong). The same holds for the fitted formula constant. This compares learners on the being's own DSL and examples; a learner outside the DSL (sequence models, symbolic regression) was not tried.

## The LLM player (Claude CLI, haiku alias): SMALL n, indicative only

Setup: World v2 small clean (6 families), budget 60 energy (10 to 12 steps), seeds 1..5 planned, at most 40 calls per world, one call per step. The model sees the menu (family index, type, difficulty, listed cost and reward, the terms of sequence and compose tasks and how many steps ahead) and its own told history (counts per (family, difficulty, strategy), the last 5 outcomes, its told reward per energy), and may answer a sequence or compose task itself (`own`: executed as `careful` by the engine, so the life stays on the engine's path; its own answer is checked afterwards against the checker for the measurement only, never sent). On a failed or invalid reply a fixed fallback plays the step (best listed reward per listed cost, careful); after 3 failures in a row the model is not asked again in that life. Wrapper `claude_player.py` (the B4b CLI flags: `--model haiku`, tools denied, one turn, no session persistence); the program runs without a shell, with the environment cleared to `PATH`, `SystemRoot`, `TEMP`, `TMP` (the minimal set of B4b), in a scratch working directory outside the repository, with a 90 s timeout (the wrapper's own limit 80 s) and a 64 KiB output cap.

What happened: the machine's disk ran nearly full during the run (under 1 GB free, from other jobs on the shared disk) and calls slowed to a median of 71 s, so the wrapper's 80 s limit failed 9 of the 25 calls of seeds 1..3 (6 in seed 2, all 3 in seed 3, which was then played by the fallback). The run was stopped during seed 4. Calls: 3 in a smoke run (seed 99, not reported), 25 in seeds 1..3 (16 valid, 0 invalid, 9 failed), 3 in seed 4 before the stop (not scored): 31 model calls in total. Latency over the 25 scored calls: median 71.0 s, maximum 92.9 s; 29 minutes of wall time for seeds 1..3.

| policy (the same 3 worlds, budget 60) | true reward per energy, mean [95% t interval, df 2] | LLM minus policy (better/worse/tie) |
|---|---|---|
| LLM player (haiku) | 0.656 [-1.001, 2.312] | - |
| LLM player with its own answers counted (per-step counterfactual) | 1.200 [-1.383, 3.783] | - |
| ours: learner (current) | 0.457 [-0.908, 1.822] | +0.198 [-2.249, +2.646] 1/2/0 |
| ours: recommended (B7 + skip fix) | 0.456 [-0.911, 1.822] | +0.200 [-2.244, +2.644] 1/2/0 |
| eps-greedy | 1.517 [-1.250, 4.283] | -0.861 [-4.594, +2.872] 1/2/0 |
| Q-learning | 1.328 [0.005, 2.650] | -0.672 [-2.990, +1.646] 1/2/0 |
| LinUCB | 1.078 [-0.624, 2.780] | -0.422 [-2.744, +1.900] 1/2/0 |
| oracle-planner (sees hidden) | 2.698 [-0.141, 5.536] | -2.042 [-4.888, +0.804] 0/3/0 |
| oracle-knows-rules (sees hidden) | 2.984 [0.854, 5.115] | -2.329 [-4.894, +0.237] 0/3/0 |

(The other methods are in the run output; every interval in this table contains 0.) Per world: seed 1, 11 valid calls, 1.333; seed 2, 5 valid calls then 3 failures in a row, 0.633; seed 3, no valid call, 0.000 (the fallback). What the model did: in 9 of its 16 valid replies it answered a sequence task itself, and all 9 answers were right by the checker (the engine executed them as `careful`, which got some of them wrong: hence the counterfactual 1.200 against 0.656); otherwise `careful` 3 times, `fast` once, skip 3 times. This is only a signal that the model can answer these sequence tasks; with 3 worlds of 11 steps, failures caused by the machine, and a budget far too short for any of the learners to learn (they are still exploring at 60 energy), it says nothing about how the LLM compares with the learners at their usual budgets.

Privacy: requests are built only by `vs_world/llm.rs::request` from synthetic features; the test `the_llm_request_holds_only_the_synthetic_task_features` asserts the exact key sets at every level and that every string comes from a fixed vocabulary (type names, strategy names, result words, the instruction), with no path, `Users`, `@`, `HOME` or `sk-`. The harness scanned every request before sending it (a hit would block the call) and every reply before writing it, for the home path in its three forms (single backslash, doubled backslash as it appears inside JSON, slash), the user name and credential words: 0 hits in requests, 0 in replies, 0 redactions; replies are redacted for both path forms before they are written. The call records (request as sent, redacted reply) went only to a scratch directory outside the repository; no run data is committed. Not visible to the scan, as in B4b: the CLI's own system prompt, the working directory, date and platform it adds, and that the service is reached with the owner's account.

## Where ours wins and where it loses

| world | ours versus the best standard method | ours versus the references that see hidden information |
|---|---|---|
| World v2 | current: loses (-0.18 to -0.38, interval below 0 in 11 of 12 conditions); recommended: ties (-0.09 to +0.13, none significant); recommended beats UCB1-tuned and KL-UCB (10 of 12 conditions), eps-greedy (7), Thompson (6), EXP3, Q, SARSA, MLP (12) | recommended 0.19 to 0.57 below oracle-knows-rules |
| first world (B2 families) | loses at budget 600 (LinUCB -0.45 [-0.66, -0.24], also eps-greedy, Thompson); ties at 3000 (LinUCB +0.007) and beats the rest | 0.33 below oracle with perfect rules at 3000, 1.13 at 600 |
| World L | B1 below the best method in all 8 pairs (-0.07 to -0.21, 3 with the interval below 0); lookahead pure d3 wins in `keys` (+0.34 [+0.20, +0.49] at 1600), loses in probe 400 (-0.21) | best of ours 0.43 to 0.64 below oracle DP in `keys` and `probe` |
| World O | current beats every method: +0.54 [+0.38, +0.70] over the best (Thompson), 20/0/0, and +0.33 over the oracle-planner; `fix+b7` -0.21 below current but still above every method | current 0.52 [0.38, 0.66] below oracle-knows-rules over the run; 0.81 in the last window |
| rule learning (same DSL and examples) | ahead of brute-force enumeration (+0.10 to +0.19 right share, clean), k-NN and trees (+0.26 to +0.74), 0 wrong answers; under noise only with the B7 robust guard | - |
| LLM (n = 3, budget 60) | no difference measurable; the model answered 9 of 9 sequence tasks right by itself | - |

## Not measured

* Methods given more than the being's planner has: no standard method learns rules while playing (none has the `discovered` strategy or sees sequence terms); a method combining LinUCB-style sharing with the B7 rule use was not built.
* Deep RL with more capacity or tuning than one 32-unit layer and two passes of grid search; policy-gradient methods; Bayesian optimisation of the hyper-parameters; re-tuning the being's own constants.
* MCTS in World O and the first world (only World v2 and World L have the true-simulator reference); MCTS with more iterations.
* World O with methods tuned on World O; World O beyond 600,000 energy units (Track B11 went to 1.2M); World O with noise.
* The LLM player beyond 3 worlds and a budget of 60, in other conditions or worlds, with other prompts or models, or on a machine that was not overloaded; the LLM's own answers inside the engine (the per-step counterfactual is not a played life).
* Rule learners outside the being's DSL (sequence models, symbolic regression, program synthesis over a larger grammar).
* Real data of any kind.

## Tests (not measurements)

`kore-self/src/being/vs_world/tests.rs`: every method (representatives of both tuning passes, including the average-reward variants) is deterministic in World v2, World L and the first world (same seed, same decisions and totals; a different seed changes a random method's choices), and so is the MCTS reference; every method spends exactly the given budget in all three worlds, and the being's learner the same budget less at most one option's energy (it stops when its best option is unaffordable and its skip rule does not fire); the methods' files (`core.rs`, `adapt.rs`, `rules.rs`) and the LLM request builder name no hidden world parameter or ground truth (source scan of 24 names; the MCTS file states it uses the true simulator); the LLM request key sets and string vocabulary; LLM replies are validated strictly (unknown slot, unaffordable option, `own` on a task that is not a sequence, unknown strategy, no JSON: refused); the LLM player stops after 3 failures in a row and respects its call budget; `guarded_search` with the B2 fit accepts exactly the forms of `discover::search` on World v2 examples; brute-force enumeration finds integer rules, refuses what it cannot pin down and is never wrong on its own accepted examples; the numeric predictors are exact on a constant-difference family.
