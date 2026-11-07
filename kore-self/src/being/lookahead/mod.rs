//! Track B8 (opt-in): planning ahead with the being's own learned model.
//!
//! * `plan.rs`    the bounded lookahead planner (depth-limited, Bayes-adaptive expectimax with sparse
//!                menu sampling over the learned model, node cap, iterative deepening);
//! * `ctl.rs`     the owner's switches in controller.txt (`lookahead`, `max_lookahead_nodes`,
//!                `max_lookahead_depth`; default off, fail closed);
//! * `worldl.rs`  World L: seeded settings where one choice changes later options (keys/vaults, a probe
//!                before an expensive job, large versus small offers) plus a control, with an exact
//!                dynamic-programming oracle;
//! * `beingl.rs`  the World L being: log-folded state and model, the planners compared, engine, logged run;
//! * `adapt.rs`   the same planner on the first menu world and on World v2 (controls).
//!
//! Nothing here changes an existing world, model or planner: with lookahead off every chooser is the
//! existing B1 planner. Author-defined simulation; the only claims are the numbers in docs/measured.md
//! (section "Track B8: planning ahead").

pub mod adapt;
pub mod beingl;
pub mod ctl;
pub mod plan;
pub mod worldl;
#[cfg(test)]
mod tests;
