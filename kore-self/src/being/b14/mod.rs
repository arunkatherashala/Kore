//! Track B14: a persistent rule library and always-on curiosity (first world; opt-in, default off).
//!
//! * `library.rs`: discovered and verified rules kept in the being's own log as library entries,
//!   indexed by context (signature), reused when a context appears for the first time (in a later
//!   life or a new domain) after a reduced check on the being's own examples, retired when the
//!   revealed answers turn against them; every reuse and retirement is a note plus a revision that
//!   cites the events that justify it.
//! * `curiosity.rs`: a small exploration budget (a share of the energy spent so far, so it never
//!   runs out in a long life) spent on the option with the highest learning value (expected
//!   improvement of the estimate, prediction error of the rule book on a signature).
//! * `ctl.rs`: the owner's switches in controller.txt (fail closed, default off).
//!
//! Synthetic world, author-defined families and rewards; the claims are the measured numbers in
//! docs/measured.md (Track B14), nothing more.

pub mod ctl;
pub mod curiosity;
pub mod library;
pub mod measure;
#[cfg(test)]
mod tests;

/// Key of a decision rationale that lists extra events the decision rests on (a library rule's note
/// and the revealed examples of its record, or the examples behind a curiosity value); the engine
/// adds them to the decision revision's `caused_by`.
pub const CITES_KEY: &str = "b14_cites";
