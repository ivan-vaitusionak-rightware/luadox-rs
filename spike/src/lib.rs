//! Phase 1 parser spike, as a library so its behaviour can be tested.
//!
//! Only what a parser can decide lives here: doc blocks, the declarations they attach
//! to, and what tree-sitter could not fit. Names, scopes and rendering are out of scope.

pub mod lua;
