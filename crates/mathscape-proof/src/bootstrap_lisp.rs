//! R32.1 — tatara-lisp bridge for BootstrapCycleSpec.
//!
//! Lifts `BootstrapCycleSpec` into/out of a tatara-lisp `Sexp`
//! form. Pairs with R10.1's `policy_to_sexp` and R12.1's
//! `primitive_to_sexp` — same pattern, same level of fidelity.
//!
//! Together with the core executor (`execute_spec_core`) this
//! closes the LOOP:
//!
//! ```text
//!   (bootstrap-spec ...) Sexp ──┐
//!                                ├─→ execute_spec → outcome
//!                                │                     │
//!   (policy ...) Sexp  ←─────────┘                     │
//!   (trained model in Lisp)  ←──────────────────────────┘
//! ```
//!
//! Input = Lisp recipe. Output = Lisp-describable model. The Rust
//! layer between them is the EXECUTOR — a finite-step driver
//! that resolves layer names and runs the iterations. The SPEC
//! and the MODEL are both pure Lisp values; the EXECUTION is
//! Rust.
//!
//! This is "fully Lisp-describable AND fully Lisp-producible" in
//! the operational sense: a Lisp program can author a spec, pass
//! it through the executor, receive a Lisp-form model. The
//! entire cycle's specification and result are in Lisp.

use crate::policy_lisp::{policy_from_sexp, policy_to_sexp};
use mathscape_core::bootstrap::BootstrapCycleSpec;
use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
use tatara_lisp::ast::{Atom, Sexp};

/// Convert a `BootstrapCycleSpec` to its canonical Sexp form.
///
/// Shape:
/// ```text
///   (bootstrap-spec
///     :corpus-generator "default"
///     :law-extractor    "derived-laws"
///     :model-updater    "default"
///     :deduper          "canonical"
///     :n-iterations     5
///     :seed-library     ((rule :name "..." :lhs ... :rhs ...) ...)
///     :seed-policy      (policy :generation 0 ...))
/// ```
#[must_use]
pub fn spec_to_sexp(spec: &BootstrapCycleSpec) -> Sexp {
    let mut items: Vec<Sexp> = vec![
        Sexp::symbol("bootstrap-spec"),
        Sexp::keyword("corpus-generator"),
        Sexp::string(spec.corpus_generator.clone()),
        Sexp::keyword("law-extractor"),
        Sexp::string(spec.law_extractor.clone()),
        Sexp::keyword("model-updater"),
        Sexp::string(spec.model_updater.clone()),
        Sexp::keyword("deduper"),
        Sexp::string(spec.deduper.clone()),
        Sexp::keyword("n-iterations"),
        Sexp::int(spec.n_iterations as i64),
        Sexp::keyword("seed-library"),
    ];
    items.push(Sexp::List(
        spec.seed_library
            .iter()
            .map(rule_to_sexp)
            .collect(),
    ));
    items.push(Sexp::keyword("seed-policy"));
    items.push(policy_to_sexp(&spec.seed_policy));
    // R37: early-stop window. Omitted from the Sexp when None to
    // keep existing recipes backward-compatible; emitted as an
    // integer keyword when Some(W).
    if let Some(w) = spec.early_stop_after_stable {
        items.push(Sexp::keyword("early-stop-after-stable"));
        items.push(Sexp::int(w as i64));
    }
    Sexp::List(items)
}

/// Parse a `BootstrapCycleSpec` from Sexp form. Returns `None` on
/// malformed input.
#[must_use]
pub fn spec_from_sexp(sexp: &Sexp) -> Option<BootstrapCycleSpec> {
    let items = match sexp {
        Sexp::List(xs) if xs.len() >= 15 => xs,
        _ => return None,
    };
    match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) if s == "bootstrap-spec" => {}
        _ => return None,
    }
    let fields = parse_kv_pairs(&items[1..])?;
    Some(BootstrapCycleSpec {
        corpus_generator: fields.get_string("corpus-generator")?,
        law_extractor: fields.get_string("law-extractor")?,
        model_updater: fields.get_string("model-updater")?,
        deduper: fields.get_string("deduper")?,
        n_iterations: fields.get_int("n-iterations")? as usize,
        seed_library: fields
            .get("seed-library")
            .and_then(|s| match s {
                Sexp::List(xs) => Some(
                    xs.iter().filter_map(rule_from_sexp).collect(),
                ),
                _ => None,
            })?,
        seed_policy: fields
            .get("seed-policy")
            .and_then(policy_from_sexp)?,
        // R37: optional; absent means None (backward-compatible).
        early_stop_after_stable: fields
            .get_int("early-stop-after-stable")
            .map(|n| n as usize),
    })
}

// ── RewriteRule <-> Sexp ────────────────────────────────────────
//
// RewriteRule isn't explicitly Lisp-bridged elsewhere (the
// kernel emits rules via discovery; they don't typically need
// Lisp round-trip). For specs with seed libraries we need it.

fn rule_to_sexp(r: &RewriteRule) -> Sexp {
    Sexp::List(vec![
        Sexp::symbol("rule"),
        Sexp::keyword("name"),
        Sexp::string(r.name.clone()),
        Sexp::keyword("lhs"),
        term_to_sexp(&r.lhs),
        Sexp::keyword("rhs"),
        term_to_sexp(&r.rhs),
    ])
}

fn rule_from_sexp(sexp: &Sexp) -> Option<RewriteRule> {
    let items = match sexp {
        Sexp::List(xs) if xs.len() == 7 => xs,
        _ => return None,
    };
    match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) if s == "rule" => {}
        _ => return None,
    }
    let fields = parse_kv_pairs(&items[1..])?;
    Some(RewriteRule {
        name: fields.get_string("name")?,
        lhs: fields.get("lhs").and_then(term_from_sexp)?,
        rhs: fields.get("rhs").and_then(term_from_sexp)?,
    })
}

/// Minimalist Term<->Sexp. Covers the subset that rules use in
/// practice — Apply, Var, Number(Nat/Int), Symbol. Does NOT handle
/// Fn or Point (not expected in rule shapes) and Tensor/
/// FloatTensor (large; bincode-persist them separately if you
/// really need Lisp round-trip of tensor-data-laden rules).
fn term_to_sexp(t: &Term) -> Sexp {
    use mathscape_core::value::Value;
    match t {
        Term::Var(v) => Sexp::List(vec![
            Sexp::symbol("var"),
            Sexp::int(*v as i64),
        ]),
        Term::Number(Value::Nat(n)) => Sexp::List(vec![
            Sexp::symbol("nat"),
            Sexp::int(*n as i64),
        ]),
        Term::Number(Value::Int(n)) => Sexp::List(vec![
            Sexp::symbol("int"),
            Sexp::int(*n),
        ]),
        Term::Number(Value::Float(bits)) => Sexp::List(vec![
            Sexp::symbol("float"),
            Sexp::float(f64::from_bits(*bits)),
        ]),
        Term::Number(Value::Tensor { .. })
        | Term::Number(Value::FloatTensor { .. }) => {
            // Tensors in rule position are rare; emit an opaque
            // marker. Callers who need tensor round-trip should
            // bincode-persist these rules separately.
            Sexp::symbol("tensor-opaque")
        }
        Term::Point(p) => Sexp::List(vec![
            Sexp::symbol("point"),
            Sexp::int(*p as i64),
        ]),
        Term::Apply(head, args) => {
            let mut items = vec![Sexp::symbol("apply"), term_to_sexp(head)];
            for a in args {
                items.push(term_to_sexp(a));
            }
            Sexp::List(items)
        }
        Term::Symbol(id, args) => {
            let mut items = vec![
                Sexp::symbol("sym"),
                Sexp::int(*id as i64),
            ];
            for a in args {
                items.push(term_to_sexp(a));
            }
            Sexp::List(items)
        }
        Term::Fn(_, _) => Sexp::symbol("fn-opaque"),
    }
}

fn term_from_sexp(sexp: &Sexp) -> Option<Term> {
    use mathscape_core::value::Value;
    let items = match sexp {
        Sexp::List(xs) if !xs.is_empty() => xs,
        _ => return None,
    };
    let tag = match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) => s,
        _ => return None,
    };
    match tag.as_str() {
        "var" => {
            let id = int_val(&items[1])?;
            Some(Term::Var(id as u32))
        }
        "nat" => {
            let n = int_val(&items[1])?;
            Some(Term::Number(Value::Nat(n as u64)))
        }
        "int" => {
            let n = int_val(&items[1])?;
            Some(Term::Number(Value::Int(n)))
        }
        "float" => {
            let f = match &items[1] {
                Sexp::Atom(Atom::Float(f)) => *f,
                Sexp::Atom(Atom::Int(n)) => *n as f64,
                _ => return None,
            };
            Value::from_f64(f).map(Term::Number)
        }
        "point" => Some(Term::Point(int_val(&items[1])? as u64)),
        "apply" => {
            if items.len() < 2 {
                return None;
            }
            let head = term_from_sexp(&items[1])?;
            let args: Option<Vec<Term>> =
                items[2..].iter().map(term_from_sexp).collect();
            Some(Term::Apply(Box::new(head), args?))
        }
        "sym" => {
            let id = int_val(&items[1])? as u32;
            let args: Option<Vec<Term>> =
                items[2..].iter().map(term_from_sexp).collect();
            Some(Term::Symbol(id, args?))
        }
        _ => None,
    }
}

fn int_val(s: &Sexp) -> Option<i64> {
    match s {
        Sexp::Atom(Atom::Int(n)) => Some(*n),
        _ => None,
    }
}

// ── Key-value parsing helpers ──────────────────────────────────

struct KvPairs<'a> {
    pairs: Vec<(String, &'a Sexp)>,
}

impl<'a> KvPairs<'a> {
    fn get(&self, key: &str) -> Option<&'a Sexp> {
        self.pairs.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    fn get_string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Sexp::Atom(Atom::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }

    fn get_int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Sexp::Atom(Atom::Int(n)) => Some(*n),
            _ => None,
        }
    }
}

fn parse_kv_pairs(items: &[Sexp]) -> Option<KvPairs<'_>> {
    if items.len() % 2 != 0 {
        return None;
    }
    let mut pairs = Vec::new();
    let mut i = 0;
    while i + 1 < items.len() {
        let key = match &items[i] {
            Sexp::Atom(Atom::Keyword(k)) => k.clone(),
            _ => return None,
        };
        pairs.push((key, &items[i + 1]));
        i += 2;
    }
    Some(KvPairs { pairs })
}

// ── R33: ExperimentScenario <-> Sexp ────────────────────────────

/// Convert an `ExperimentScenario` to Sexp form:
/// ```text
///   (experiment :name "..."
///     :phases ((bootstrap-spec ...) (bootstrap-spec ...) ...))
/// ```
#[must_use]
pub fn scenario_to_sexp(
    scenario: &mathscape_core::bootstrap::ExperimentScenario,
) -> Sexp {
    Sexp::List(vec![
        Sexp::symbol("experiment"),
        Sexp::keyword("name"),
        Sexp::string(scenario.name.clone()),
        Sexp::keyword("phases"),
        Sexp::List(scenario.phases.iter().map(spec_to_sexp).collect()),
    ])
}

/// Parse an `ExperimentScenario` from Sexp. `None` on malformed.
#[must_use]
pub fn scenario_from_sexp(
    sexp: &Sexp,
) -> Option<mathscape_core::bootstrap::ExperimentScenario> {
    let items = match sexp {
        Sexp::List(xs) if xs.len() == 5 => xs,
        _ => return None,
    };
    match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) if s == "experiment" => {}
        _ => return None,
    }
    let fields = parse_kv_pairs(&items[1..])?;
    Some(mathscape_core::bootstrap::ExperimentScenario {
        name: fields.get_string("name")?,
        phases: fields
            .get("phases")
            .and_then(|s| match s {
                Sexp::List(xs) => Some(xs.iter().filter_map(spec_from_sexp).collect()),
                _ => None,
            })?,
    })
}

// ── Phase U.4: LearningObservation <-> Sexp ─────────────────────
//
// Closes the "everything in the meta-loop is a Lisp value" claim.
// With R10 policy, R32 spec, R33 scenario, and now U.1 observation
// all round-tripping through Sexp, the outer loop
//   (Sexp scenario, Sexp observation_history) → Sexp next_scenario
// is a pure function. Its signature is the WASM module signature
// the Phase U.5 virtualization target will export.

/// Convert a `LearningObservation` to Sexp form:
/// ```text
///   (observation
///     :total-library-size  7
///     :seed-library-size   2
///     :net-growth-per-phase (3 2 1 0)
///     :saturation-phase-index 3        ; or nil
///     :extract-ns-per-iteration (100 200 300)
///     :trained-policy-delta-norm 0.42
///     :scenario-total-ns   123456
///     :chain-attestation   "hex...")
/// ```
#[must_use]
pub fn observation_to_sexp(
    obs: &mathscape_core::bootstrap::LearningObservation,
) -> Sexp {
    let mut items: Vec<Sexp> = vec![
        Sexp::symbol("observation"),
        Sexp::keyword("total-library-size"),
        Sexp::int(obs.total_library_size as i64),
        Sexp::keyword("seed-library-size"),
        Sexp::int(obs.seed_library_size as i64),
        Sexp::keyword("net-growth-per-phase"),
        Sexp::List(
            obs.net_growth_per_phase
                .iter()
                .map(|n| Sexp::int(*n as i64))
                .collect(),
        ),
        Sexp::keyword("saturation-phase-index"),
    ];
    match obs.saturation_phase_index {
        Some(i) => items.push(Sexp::int(i as i64)),
        None => items.push(Sexp::symbol("nil")),
    }
    items.push(Sexp::keyword("extract-ns-per-iteration"));
    items.push(Sexp::List(
        obs.extract_ns_per_iteration
            .iter()
            .map(|n| Sexp::int(*n as i64))
            .collect(),
    ));
    items.push(Sexp::keyword("trained-policy-delta-norm"));
    items.push(Sexp::float(obs.trained_policy_delta_norm));
    items.push(Sexp::keyword("scenario-total-ns"));
    items.push(Sexp::int(obs.scenario_total_ns as i64));
    items.push(Sexp::keyword("chain-attestation"));
    items.push(Sexp::string(hex_encode(obs.chain_attestation.as_bytes())));
    Sexp::List(items)
}

/// Parse a `LearningObservation` from Sexp. `None` on malformed.
#[must_use]
pub fn observation_from_sexp(
    sexp: &Sexp,
) -> Option<mathscape_core::bootstrap::LearningObservation> {
    let items = match sexp {
        Sexp::List(xs) if xs.len() >= 17 => xs,
        _ => return None,
    };
    match &items[0] {
        Sexp::Atom(Atom::Symbol(s)) if s == "observation" => {}
        _ => return None,
    }
    let fields = parse_kv_pairs(&items[1..])?;

    let total_library_size = fields.get_int("total-library-size")? as usize;
    let seed_library_size = fields.get_int("seed-library-size")? as usize;
    let net_growth_per_phase: Vec<usize> = match fields.get("net-growth-per-phase")? {
        Sexp::List(xs) => xs
            .iter()
            .filter_map(|x| match x {
                Sexp::Atom(Atom::Int(n)) => Some(*n as usize),
                _ => None,
            })
            .collect(),
        _ => return None,
    };
    let saturation_phase_index = match fields.get("saturation-phase-index")? {
        Sexp::Atom(Atom::Int(n)) => Some(*n as usize),
        Sexp::Atom(Atom::Symbol(s)) if s == "nil" => None,
        _ => return None,
    };
    let extract_ns_per_iteration: Vec<u64> =
        match fields.get("extract-ns-per-iteration")? {
            Sexp::List(xs) => xs
                .iter()
                .filter_map(|x| match x {
                    Sexp::Atom(Atom::Int(n)) => Some(*n as u64),
                    _ => None,
                })
                .collect(),
            _ => return None,
        };
    let trained_policy_delta_norm = match fields.get("trained-policy-delta-norm")?
    {
        Sexp::Atom(Atom::Float(f)) => *f,
        Sexp::Atom(Atom::Int(n)) => *n as f64,
        _ => return None,
    };
    let scenario_total_ns = fields.get_int("scenario-total-ns")? as u64;
    let hex = fields.get_string("chain-attestation")?;
    let bytes = hex_decode(&hex)?;
    // TermRef holds the 32-byte hash directly; `from_bytes` would
    // re-hash the input instead of using it as the hash. Construct
    // the pub-field tuple-struct directly.
    let arr: [u8; 32] = bytes.as_slice().try_into().ok()?;
    let chain_attestation = mathscape_core::hash::TermRef(arr);

    Some(mathscape_core::bootstrap::LearningObservation {
        total_library_size,
        seed_library_size,
        net_growth_per_phase,
        saturation_phase_index,
        extract_ns_per_iteration,
        trained_policy_delta_norm,
        scenario_total_ns,
        chain_attestation,
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::policy::LinearPolicy;

    #[test]
    fn default_m0_spec_roundtrips_via_sexp() {
        let spec = BootstrapCycleSpec::default_m0();
        let sexp = spec_to_sexp(&spec);
        let back = spec_from_sexp(&sexp).expect("valid sexp parses");
        assert_eq!(spec, back);
    }

    #[test]
    fn spec_with_seed_library_roundtrips() {
        use mathscape_core::value::Value;
        let r = RewriteRule {
            name: "seed".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        };
        let mut spec = BootstrapCycleSpec::default_m0();
        spec.seed_library = vec![r];
        let sexp = spec_to_sexp(&spec);
        let back = spec_from_sexp(&sexp).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn spec_with_trained_policy_roundtrips() {
        let mut spec = BootstrapCycleSpec::default_m0();
        // Replace the seed with a hypothetical already-trained policy.
        let mut p = LinearPolicy::tensor_seeking_prior();
        p.bias = 0.7;
        p.trained_steps = 100;
        p.generation = 3;
        spec.seed_policy = p;
        let sexp = spec_to_sexp(&spec);
        let back = spec_from_sexp(&sexp).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn malformed_sexp_returns_none() {
        assert!(spec_from_sexp(&Sexp::int(42)).is_none());
        assert!(
            spec_from_sexp(&Sexp::List(vec![Sexp::symbol("wrong-head")]))
                .is_none()
        );
    }

    // ── R33 scenario Sexp tests ───────────────────────────────────

    #[test]
    fn scenario_roundtrips_via_sexp() {
        use mathscape_core::bootstrap::{BootstrapCycleSpec, ExperimentScenario};
        let base = BootstrapCycleSpec::default_m0();
        let scenario = ExperimentScenario {
            name: "multi-phase-test".into(),
            phases: vec![base.clone(), base.clone(), base],
        };
        let sexp = scenario_to_sexp(&scenario);
        let back = scenario_from_sexp(&sexp).expect("valid scenario sexp");
        assert_eq!(scenario, back);
    }

    #[test]
    fn empty_scenario_roundtrips_via_sexp() {
        use mathscape_core::bootstrap::ExperimentScenario;
        let scenario = ExperimentScenario {
            name: "empty".into(),
            phases: Vec::new(),
        };
        let sexp = scenario_to_sexp(&scenario);
        let back = scenario_from_sexp(&sexp).unwrap();
        assert_eq!(scenario, back);
    }

    #[test]
    fn malformed_scenario_sexp_returns_none() {
        assert!(scenario_from_sexp(&Sexp::int(42)).is_none());
        assert!(
            scenario_from_sexp(&Sexp::List(vec![Sexp::symbol("not-experiment")]))
                .is_none()
        );
    }

    // ── Phase U.4: observation Sexp tests ────────────────────────

    #[test]
    fn observation_roundtrips_via_sexp() {
        use mathscape_core::bootstrap::LearningObservation;
        let obs = LearningObservation {
            total_library_size: 7,
            seed_library_size: 2,
            net_growth_per_phase: vec![3, 2, 1, 0],
            saturation_phase_index: Some(3),
            extract_ns_per_iteration: vec![100, 200, 300, 400],
            trained_policy_delta_norm: 0.4242,
            scenario_total_ns: 1_234_567,
            chain_attestation: mathscape_core::hash::TermRef::from_bytes(
                b"phase-u-observation-rt",
            ),
        };
        let sexp = observation_to_sexp(&obs);
        let back = observation_from_sexp(&sexp).expect("valid observation sexp");
        assert_eq!(obs, back);
    }

    #[test]
    fn observation_with_no_saturation_roundtrips() {
        use mathscape_core::bootstrap::LearningObservation;
        let obs = LearningObservation {
            total_library_size: 5,
            seed_library_size: 0,
            net_growth_per_phase: vec![5],
            saturation_phase_index: None,
            extract_ns_per_iteration: vec![],
            trained_policy_delta_norm: 0.0,
            scenario_total_ns: 0,
            chain_attestation: mathscape_core::hash::TermRef::from_bytes(b""),
        };
        let sexp = observation_to_sexp(&obs);
        let back = observation_from_sexp(&sexp).unwrap();
        assert_eq!(obs, back);
        assert!(back.saturation_phase_index.is_none());
    }

    #[test]
    fn malformed_observation_sexp_returns_none() {
        assert!(observation_from_sexp(&Sexp::int(42)).is_none());
        assert!(
            observation_from_sexp(&Sexp::List(vec![Sexp::symbol("not-observation")]))
                .is_none()
        );
    }

    #[test]
    fn observation_from_sexp_handles_policy_delta_as_int() {
        // If the delta norm happens to be an integer (e.g. 0), the
        // parser should accept it as a float.
        use mathscape_core::bootstrap::LearningObservation;
        let obs = LearningObservation {
            total_library_size: 1,
            seed_library_size: 0,
            net_growth_per_phase: vec![1],
            saturation_phase_index: None,
            extract_ns_per_iteration: vec![],
            trained_policy_delta_norm: 0.0,
            scenario_total_ns: 100,
            chain_attestation: mathscape_core::hash::TermRef::from_bytes(b"x"),
        };
        // Emit-then-parse works.
        let sexp = observation_to_sexp(&obs);
        let back = observation_from_sexp(&sexp).unwrap();
        assert_eq!(obs, back);
    }

    #[test]
    fn hex_encode_decode_roundtrip() {
        for bytes in [
            b"".to_vec(),
            b"hello".to_vec(),
            vec![0u8, 1, 2, 255, 16, 128],
            {
                let mut v = Vec::new();
                for i in 0..32u8 {
                    v.push(i);
                }
                v
            },
        ] {
            let enc = hex_encode(&bytes);
            let dec = hex_decode(&enc).expect("valid hex round-trip");
            assert_eq!(bytes, dec);
        }
    }
}
