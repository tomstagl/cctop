//! Outcome-and-rework rules (coach PRD §5.2, Phase 6): A32 verification
//! gap, A33 commit without a check, A36 correction streak, A38 failure
//! cascade, A40 review before merge, A41 waiting on you, A42
//! natural-boundary checkpoint, A45 denial streak, A47 turn died. Until
//! they land the module contributes nothing; A15 (error loop) and A18 (no
//! hand-off) were retired in their favour.

use crate::advisor::Rule;

pub fn all() -> Vec<Box<dyn Rule>> {
    Vec::new()
}
