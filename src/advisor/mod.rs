//! The Advisor: deterministic rules that turn observed patterns into one
//! concrete change to how the user works. No model call, no network.

pub mod rules;

use std::collections::HashSet;

use crate::ui::State;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Saving {
    /// Tokens saved per remaining turn.
    Tokens(u64),
    /// Wall-clock seconds saved per remaining turn.
    Seconds(u64),
    /// Avoids a hard stop (rate limit, lost context); ranked above tokens.
    Avoids,
}

impl Saving {
    /// Ranking key: bigger is more urgent.
    pub fn rank(self) -> u64 {
        match self {
            Saving::Avoids => u64::MAX,
            // A second of waiting ≈ 2k tokens of value, so the two compare.
            Saving::Seconds(s) => s * 2_000,
            Saving::Tokens(t) => t,
        }
    }

    pub fn label(self) -> String {
        match self {
            Saving::Tokens(t) => format!("~{}/turn", crate::ui::fmt::tokens(t)),
            Saving::Seconds(s) => format!("~{}/turn", crate::ui::fmt::duration_ms(s as i64 * 1000)),
            Saving::Avoids => "avoids a hard stop".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Advice {
    pub rule: &'static str,
    pub headline: String,
    pub evidence: String,
    pub action: String,
    pub saving: Saving,
    /// Key into the explanation text ([`rules::explain`]).
    pub doc_key: &'static str,
}

pub trait Rule: Send + Sync {
    /// `A01` …
    fn id(&self) -> &'static str;
    fn evaluate(&self, state: &State) -> Option<Advice>;
}

/// Evaluates every rule and keeps the ranked, non-dismissed list.
pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
    dismissed: HashSet<&'static str>,
    /// Current ranked advice, best first.
    pub current: Vec<Advice>,
    /// Rule id → turn number it first fired (recency tiebreak).
    first_fired: std::collections::HashMap<&'static str, usize>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new(rules::all())
    }
}

impl Engine {
    pub fn new(rules: Vec<Box<dyn Rule>>) -> Engine {
        Engine {
            rules,
            dismissed: HashSet::new(),
            current: Vec::new(),
            first_fired: Default::default(),
        }
    }

    /// Re-run every rule against `state`.
    pub fn evaluate(&mut self, state: &State) {
        let turn = state.agg.turns.len();
        let mut out: Vec<Advice> = Vec::new();
        for r in &self.rules {
            if self.dismissed.contains(r.id()) {
                continue;
            }
            if let Some(a) = r.evaluate(state) {
                self.first_fired.entry(r.id()).or_insert(turn);
                out.push(a);
            } else {
                self.first_fired.remove(r.id());
            }
        }
        let ff = &self.first_fired;
        out.sort_by(|a, b| {
            b.saving
                .rank()
                .cmp(&a.saving.rank())
                .then_with(|| ff.get(b.rule).cmp(&ff.get(a.rule)))
        });
        self.current = out;
    }

    pub fn dismiss(&mut self, rule: &'static str) {
        self.dismissed.insert(rule);
        self.current.retain(|a| a.rule != rule);
    }

    pub fn rule_ids(&self) -> Vec<&'static str> {
        self.rules.iter().map(|r| r.id()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;

    #[test]
    fn empty_session_fires_nothing() {
        let mut e = Engine::default();
        e.evaluate(&State::new(Pricing::bundled()));
        assert!(e.current.is_empty());
        assert_eq!(e.rule_ids().len(), 12);
        assert_eq!(e.rule_ids()[6], "A07");
    }

    #[test]
    fn ranking_and_dismissal() {
        struct Fixed(&'static str, Saving);
        impl Rule for Fixed {
            fn id(&self) -> &'static str {
                self.0
            }
            fn evaluate(&self, _: &State) -> Option<Advice> {
                Some(Advice {
                    rule: self.0,
                    headline: self.0.into(),
                    evidence: String::new(),
                    action: String::new(),
                    saving: self.1,
                    doc_key: self.0,
                })
            }
        }
        let mut e = Engine::new(vec![
            Box::new(Fixed("T", Saving::Tokens(5_000))),
            Box::new(Fixed("S", Saving::Seconds(10))),
            Box::new(Fixed("A", Saving::Avoids)),
        ]);
        e.evaluate(&State::new(Pricing::bundled()));
        let order: Vec<_> = e.current.iter().map(|a| a.rule).collect();
        assert_eq!(order, ["A", "S", "T"]);
        e.dismiss("A");
        e.evaluate(&State::new(Pricing::bundled()));
        assert_eq!(
            e.current.iter().map(|a| a.rule).collect::<Vec<_>>(),
            ["S", "T"]
        );
    }

    /// The advisor must be pure: rules only read `State`. The sources of the
    /// engine and every rule must not reach for processes or the network.
    #[test]
    fn advisor_spawns_nothing() {
        for src in [include_str!("mod.rs"), include_str!("rules.rs")] {
            for forbidden in [
                "std::process",
                "Command::",
                "TcpStream",
                "reqwest",
                "std::net",
                "tokio::net",
            ] {
                let body = src.split("#[cfg(test)]").next().unwrap_or(src);
                assert!(!body.contains(forbidden), "advisor uses {forbidden}");
            }
        }
        // And evaluating on a real session is side-effect free (pure fn of state).
        let mut e = Engine::default();
        let s = crate::ui::state::tests_support::fixture_state();
        e.evaluate(&s);
        let a = e.current.clone();
        e.evaluate(&s);
        assert_eq!(a, e.current);
    }
}
