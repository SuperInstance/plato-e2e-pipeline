//! plato-e2e-pipeline — End-to-End DCS → Belief → Deploy Integration
//!
//! Proves the entire PLATO stack works as one system by chaining:
//!
//! ```text
//!   Tile Set + Agent Pool
//!     │
//!     ├─ per tile ──▶ DcsEngine (7-phase cycle)      [plato-dcs @ 92a3387]
//!     │                └─ verified solutions
//!     │                    └─ score_to_belief()
//!     │                        └─ BeliefScore         [plato-unified-belief @ 8e21eae]
//!     │                            └─ DeployPolicy::classify()
//!     │                                └─ DeployDecision (Live/Monitored/HumanGated)
//!     │                                               [plato-deploy-policy @ 84e2525]
//!     └─▶ E2EResult { tile_results, stats }
//! ```
//!
//! ## Key invariant (Oracle1 proven)
//! The 5.88× specialist advantage propagates end-to-end:
//!
//! `confidence = dcs_score / SPECIALIST_RATIO`
//!
//! → specialist confidence is exactly 5.88× a generalist's at equal trust.
//!
//! ## Fleet crate attribution
//! APIs below are inlined verbatim (private helpers collapsed) from:
//! - `SuperInstance/plato-dcs`            @ 92a3387  (MIT)
//! - `SuperInstance/plato-unified-belief` @ 8e21eae  (MIT)
//! - `SuperInstance/plato-deploy-policy`  @ 84e2525  (MIT)
//!
//! To use git deps instead of inlining, add to Cargo.toml:
//! ```toml
//! plato-dcs            = { git = "https://github.com/SuperInstance/plato-dcs" }
//! plato-unified-belief = { git = "https://github.com/SuperInstance/plato-unified-belief" }
//! plato-deploy-policy  = { git = "https://github.com/SuperInstance/plato-deploy-policy" }
//! ```

// ═══════════════════════════════════════════════════════════════════════════════
// § 1  INLINED: plato-dcs  (SuperInstance/plato-dcs @ 92a3387)
// ═══════════════════════════════════════════════════════════════════════════════

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static DCS_ID_SEQ: AtomicU64 = AtomicU64::new(0);

fn dcs_next_id() -> u64 {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    ns.wrapping_add(DCS_ID_SEQ.fetch_add(1, Ordering::Relaxed))
}

/// Oracle1-proven performance constants.
pub const SPECIALIST_RATIO: f64 = 5.88;
pub const DCS_FLEET_RATIO: f64 = 21.87;
const SYNTHESIS_BONUS: f64 = DCS_FLEET_RATIO / SPECIALIST_RATIO; // ≈ 3.72

/// Problem/agent domain tags.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Domain {
    Math,
    Logic,
    Language,
    Code,
    General,
}

impl std::fmt::Display for Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Domain::Math => "math",
            Domain::Logic => "logic",
            Domain::Language => "language",
            Domain::Code => "code",
            Domain::General => "general",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone)]
struct DcsTile {
    id: u64,
    content: String,
    domain: Domain,
    complexity: f64,
}

impl DcsTile {
    fn new(content: impl Into<String>, domain: Domain, complexity: f64) -> Self {
        Self { id: dcs_next_id(), content: content.into(), domain, complexity: complexity.clamp(0.0, 1.0) }
    }
}

#[derive(Debug, Clone)]
struct DcsAgent {
    name: String,
    specialty: Domain,
    trust_score: f64,
}

impl DcsAgent {
    fn new(name: impl Into<String>, specialty: Domain, trust_score: f64) -> Self {
        Self { name: name.into(), specialty, trust_score: trust_score.clamp(0.0, 1.0) }
    }

    /// 5.88× advantage on own domain; baseline trust_score elsewhere.
    fn performance_on(&self, domain: &Domain) -> f64 {
        if &self.specialty == domain { SPECIALIST_RATIO * self.trust_score } else { self.trust_score }
    }
}

#[derive(Debug, Clone)]
struct DcsSolution {
    agent_name: String,
    score: f64,
    verified: bool,
}

impl DcsSolution {
    fn compute(tile: &DcsTile, agent: &DcsAgent) -> Self {
        Self { agent_name: agent.name.clone(), score: agent.performance_on(&tile.domain), verified: false }
    }
}

#[derive(Debug, Clone)]
struct DcsAssignment {
    tile: DcsTile,
    agent: DcsAgent,
}

/// Seven DCS phases plus terminal states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Divide, Assign, Compute, Verify, Synthesize, Validate, Integrate,
    Complete,
    Failed(String),
}

impl Phase {
    fn is_terminal(&self) -> bool {
        matches!(self, Phase::Complete | Phase::Failed(_))
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Divide     => write!(f, "DIVIDE"),
            Phase::Assign     => write!(f, "ASSIGN"),
            Phase::Compute    => write!(f, "COMPUTE"),
            Phase::Verify     => write!(f, "VERIFY"),
            Phase::Synthesize => write!(f, "SYNTHESIZE"),
            Phase::Validate   => write!(f, "VALIDATE"),
            Phase::Integrate  => write!(f, "INTEGRATE"),
            Phase::Complete   => write!(f, "COMPLETE"),
            Phase::Failed(e)  => write!(f, "FAILED({e})"),
        }
    }
}

#[derive(Debug, Clone)]
struct DcsState {
    phase: Phase,
    problem: DcsTile,
    agents: Vec<DcsAgent>,
    sub_tasks: Vec<DcsTile>,
    assignments: Vec<DcsAssignment>,
    solutions: Vec<DcsSolution>,
    verified_solutions: Vec<DcsSolution>,
    validation_score: f64,
    committed: bool,
    cycle_log: Vec<String>,
}

impl DcsState {
    fn new(problem: DcsTile, agents: Vec<DcsAgent>) -> Self {
        Self {
            phase: Phase::Divide,
            problem,
            agents,
            sub_tasks: Vec::new(),
            assignments: Vec::new(),
            solutions: Vec::new(),
            verified_solutions: Vec::new(),
            validation_score: 0.0,
            committed: false,
            cycle_log: Vec::new(),
        }
    }
}

struct DcsEngine {
    verification_threshold: f64,
    validation_threshold: f64,
    min_sub_tasks: usize,
}

impl Default for DcsEngine {
    fn default() -> Self {
        Self { verification_threshold: 0.5, validation_threshold: 1.0, min_sub_tasks: 1 }
    }
}

impl DcsEngine {
    fn new() -> Self { Self::default() }

    fn with_thresholds(verification: f64, validation: f64) -> Self {
        Self { verification_threshold: verification, validation_threshold: validation, ..Self::default() }
    }

    fn divide(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Divide { return s; }
        let n = ((s.problem.complexity * 4.0).ceil() as usize).max(self.min_sub_tasks);
        let sub_c = s.problem.complexity / n as f64;
        s.sub_tasks = (0..n)
            .map(|i| DcsTile {
                id: dcs_next_id(),
                content: format!("{} [part {}/{}]", s.problem.content, i + 1, n),
                domain: s.problem.domain.clone(),
                complexity: sub_c,
            })
            .collect();
        s.cycle_log.push(format!("DIVIDE: '{}' → {} sub-tasks", s.problem.content, n));
        s.phase = Phase::Assign;
        s
    }

    fn assign(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Assign { return s; }
        if s.agents.is_empty() {
            s.phase = Phase::Failed("No agents in pool".into());
            s.cycle_log.push("ASSIGN: failed — empty agent pool".into());
            return s;
        }
        let assignments: Vec<DcsAssignment> = s.sub_tasks.iter().map(|tile| {
            let best = s.agents.iter()
                .max_by(|a, b| a.performance_on(&tile.domain)
                    .partial_cmp(&b.performance_on(&tile.domain))
                    .unwrap_or(std::cmp::Ordering::Equal))
                .expect("agents non-empty");
            DcsAssignment { tile: tile.clone(), agent: best.clone() }
        }).collect();
        s.cycle_log.push(format!("ASSIGN: {} assignments made", assignments.len()));
        s.assignments = assignments;
        s.phase = Phase::Compute;
        s
    }

    fn compute(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Compute { return s; }
        s.solutions = s.assignments.iter().map(|a| DcsSolution::compute(&a.tile, &a.agent)).collect();
        s.cycle_log.push(format!("COMPUTE: {} solutions produced", s.solutions.len()));
        s.phase = Phase::Verify;
        s
    }

    fn verify(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Verify { return s; }
        let thresh = self.verification_threshold;
        let before = s.solutions.len();
        let verified: Vec<DcsSolution> = s.solutions.drain(..)
            .map(|mut sol| { sol.verified = sol.score >= thresh; sol })
            .filter(|sol| sol.verified)
            .collect();
        if verified.is_empty() {
            s.phase = Phase::Failed(format!("All {before} solutions failed (threshold={thresh:.4})"));
            s.cycle_log.push("VERIFY: all solutions rejected".into());
            return s;
        }
        s.cycle_log.push(format!("VERIFY: {}/{} passed (threshold={:.4})", verified.len(), before, thresh));
        s.verified_solutions = verified;
        s.phase = Phase::Synthesize;
        s
    }

    fn synthesize(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Synthesize { return s; }
        let n = s.verified_solutions.len() as f64;
        let avg = s.verified_solutions.iter().map(|sol| sol.score).sum::<f64>() / n;
        s.validation_score = avg * SYNTHESIS_BONUS;
        s.cycle_log.push(format!("SYNTHESIZE: {} merged, fleet_score={:.6}", n as usize, s.validation_score));
        s.phase = Phase::Validate;
        s
    }

    fn validate(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Validate { return s; }
        if s.validation_score < self.validation_threshold {
            s.phase = Phase::Failed(format!(
                "Fleet score {:.6} < threshold {:.6}", s.validation_score, self.validation_threshold
            ));
            s.cycle_log.push("VALIDATE: failed".into());
            return s;
        }
        s.cycle_log.push(format!("VALIDATE: {:.6} ≥ {:.4}", s.validation_score, self.validation_threshold));
        s.phase = Phase::Integrate;
        s
    }

    fn integrate(&self, mut s: DcsState) -> DcsState {
        if s.phase != Phase::Integrate { return s; }
        s.committed = true;
        s.cycle_log.push(format!("INTEGRATE: committed (fleet_score={:.6})", s.validation_score));
        s.phase = Phase::Complete;
        s
    }

    fn run(&self, mut state: DcsState) -> DcsState {
        while !state.phase.is_terminal() {
            state = match &state.phase {
                Phase::Divide     => self.divide(state),
                Phase::Assign     => self.assign(state),
                Phase::Compute    => self.compute(state),
                Phase::Verify     => self.verify(state),
                Phase::Synthesize => self.synthesize(state),
                Phase::Validate   => self.validate(state),
                Phase::Integrate  => self.integrate(state),
                Phase::Complete | Phase::Failed(_) => break,
            };
        }
        state
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 2  INLINED: plato-unified-belief  (SuperInstance/plato-unified-belief @ 8e21eae)
// ═══════════════════════════════════════════════════════════════════════════════

/// Three-dimensional Bayesian belief score.
///
/// - **confidence** — quality of the DCS solution (normalised by SPECIALIST_RATIO)
/// - **trust**      — calibrated agent reliability score
/// - **relevance**  — domain-match bonus: 1.0 for specialists, 0.5 for generalists
#[derive(Debug, Clone, Copy)]
pub struct BeliefScore {
    pub confidence: f32,
    pub trust: f32,
    pub relevance: f32,
}

impl BeliefScore {
    pub fn new(confidence: f32, trust: f32, relevance: f32) -> Self {
        Self {
            confidence: confidence.clamp(0.0, 1.0),
            trust: trust.clamp(0.0, 1.0),
            relevance: relevance.clamp(0.0, 1.0),
        }
    }

    /// Geometric-mean composite ∈ [0, 1].
    /// A single zero in any dimension collapses the whole to zero.
    pub fn composite(&self) -> f32 {
        (self.confidence * self.trust * self.relevance).powf(1.0 / 3.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 3  INLINED: plato-deploy-policy  (SuperInstance/plato-deploy-policy @ 84e2525)
// ═══════════════════════════════════════════════════════════════════════════════

/// Deployment tier — routes changes based on risk and belief strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Auto-deploy, A/B testing, instant rollback. Cost of failure: LOW.
    Live,
    /// Shadow mode, graduated rollout. Cost of failure: MEDIUM.
    Monitored,
    /// Simulation first, human approval required. Cost of failure: HIGH.
    HumanGated,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Live       => "live",
            Tier::Monitored  => "monitored",
            Tier::HumanGated => "human-gated",
        }
    }
}

/// Outcome of classifying one tile through deploy policy.
#[derive(Debug, Clone)]
pub struct DeployDecision {
    pub tier: Tier,
    pub composite_score: f32,
    pub reason: String,
    pub requires_human: bool,
    pub rollout_pct: u8,
}

impl DeployDecision {
    pub fn is_auto(&self) -> bool { !self.requires_human }
}

struct DeployPolicy {
    live_threshold: f32,
    human_threshold: f32,
    monitored_start_pct: u8,
    absolute_min_confidence: f32,
    absolute_min_trust: f32,
}

impl Default for DeployPolicy {
    fn default() -> Self {
        Self {
            live_threshold: 0.8,
            human_threshold: 0.5,
            monitored_start_pct: 5,
            absolute_min_confidence: 0.3,
            absolute_min_trust: 0.3,
        }
    }
}

impl DeployPolicy {
    fn new(live: f32, human: f32) -> Self {
        Self { live_threshold: live, human_threshold: human, ..Self::default() }
    }

    fn classify(&self, confidence: f32, trust: f32, relevance: f32) -> DeployDecision {
        let composite = (confidence * trust * relevance).powf(1.0 / 3.0);

        if confidence < self.absolute_min_confidence {
            return DeployDecision {
                tier: Tier::HumanGated, composite_score: composite,
                reason: format!("confidence {confidence:.3} below floor {:.2}", self.absolute_min_confidence),
                requires_human: true, rollout_pct: 0,
            };
        }
        if trust < self.absolute_min_trust {
            return DeployDecision {
                tier: Tier::HumanGated, composite_score: composite,
                reason: format!("trust {trust:.3} below floor {:.2}", self.absolute_min_trust),
                requires_human: true, rollout_pct: 0,
            };
        }

        if composite >= self.live_threshold {
            DeployDecision {
                tier: Tier::Live, composite_score: composite,
                reason: format!("composite {composite:.4} ≥ live {:.1}", self.live_threshold),
                requires_human: false, rollout_pct: 100,
            }
        } else if composite >= self.human_threshold {
            DeployDecision {
                tier: Tier::Monitored, composite_score: composite,
                reason: format!("composite {composite:.4} in monitored [{:.1}, {:.1})",
                    self.human_threshold, self.live_threshold),
                requires_human: false, rollout_pct: self.monitored_start_pct,
            }
        } else {
            DeployDecision {
                tier: Tier::HumanGated, composite_score: composite,
                reason: format!("composite {composite:.4} < human {:.1}", self.human_threshold),
                requires_human: true, rollout_pct: 0,
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 4  PUBLIC PIPELINE API
// ═══════════════════════════════════════════════════════════════════════════════

/// One tile (sub-problem / knowledge unit) to run through the pipeline.
#[derive(Debug, Clone)]
pub struct TileInput {
    pub content: String,
    pub domain: Domain,
    /// Normalised complexity ∈ [0.0, 1.0]; drives DCS DIVIDE fan-out.
    pub complexity: f64,
}

impl TileInput {
    pub fn new(content: impl Into<String>, domain: Domain, complexity: f64) -> Self {
        Self { content: content.into(), domain, complexity }
    }
}

/// One agent in the pool.
#[derive(Debug, Clone)]
pub struct AgentInput {
    pub name: String,
    pub specialty: Domain,
    /// Calibrated trust ∈ [0.0, 1.0].
    pub trust_score: f64,
}

impl AgentInput {
    pub fn new(name: impl Into<String>, specialty: Domain, trust_score: f64) -> Self {
        Self { name: name.into(), specialty, trust_score }
    }
}

// ── Per-tile result ───────────────────────────────────────────────────────────

/// Belief + deploy outcome for one tile after a full DCS cycle.
#[derive(Debug, Clone)]
pub struct TileResult {
    pub tile_content: String,
    /// Name of the winning agent (empty if DCS failed before ASSIGN).
    pub agent_name: String,
    /// `true` when the agent's specialty matches the tile's domain.
    pub is_specialist: bool,
    /// Raw DCS solution score (0.0 on total failure).
    pub dcs_score: f64,
    pub belief: BeliefScore,
    pub decision: DeployDecision,
}

// ── Pipeline stats ────────────────────────────────────────────────────────────

/// Aggregate statistics across all tiles in one pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineStats {
    pub total_tiles: usize,
    /// Tiles whose DCS cycle produced a non-zero score.
    pub verified_tiles: usize,
    pub tier_live: usize,
    pub tier_monitored: usize,
    pub tier_human_gated: usize,
    pub avg_belief_composite: f32,
    /// Average `confidence` for specialist-assigned tiles (≈ agent trust_score).
    pub specialist_avg_confidence: f32,
    /// Average `confidence` for generalist-assigned tiles (≈ trust_score / 5.88).
    pub generalist_avg_confidence: f32,
    pub processing_time_ns: u64,
}

// ── E2E result ────────────────────────────────────────────────────────────────

/// Full result of one pipeline run across all input tiles.
#[derive(Debug)]
pub struct E2EResult {
    pub problem_desc: String,
    /// One entry per input tile, in input order.
    pub tile_results: Vec<TileResult>,
    pub stats: PipelineStats,
}

impl E2EResult {
    /// Fraction of tiles that reached the Live tier.
    pub fn live_ratio(&self) -> f64 {
        if self.stats.total_tiles == 0 { return 0.0; }
        self.stats.tier_live as f64 / self.stats.total_tiles as f64
    }

    /// Specialist / generalist confidence ratio (should ≈ SPECIALIST_RATIO = 5.88).
    pub fn specialist_confidence_ratio(&self) -> Option<f32> {
        let g = self.stats.generalist_avg_confidence;
        let s = self.stats.specialist_avg_confidence;
        if g == 0.0 || s == 0.0 { return None; }
        Some(s / g)
    }
}

// ── E2E Pipeline ──────────────────────────────────────────────────────────────

/// End-to-end pipeline: DCS → unified-belief → deploy-policy.
///
/// Runs one independent DCS cycle per `TileInput`. Verified solutions are
/// converted to `BeliefScore` and classified into a deployment `Tier`.
/// Failed DCS runs produce `HumanGated` outcomes with belief (0.1, 0.1, 0.1).
pub struct E2EPipeline {
    dcs: DcsEngine,
    policy: DeployPolicy,
}

impl Default for E2EPipeline {
    fn default() -> Self { Self::new() }
}

impl E2EPipeline {
    /// Pipeline with default thresholds:
    /// DCS verify=0.5 | validate=1.0 · Belief live=0.8 | human=0.5
    pub fn new() -> Self {
        Self { dcs: DcsEngine::new(), policy: DeployPolicy::default() }
    }

    /// Pipeline with custom thresholds (for boundary-condition tests).
    pub fn with_thresholds(
        verify_threshold: f64,
        validate_threshold: f64,
        live_belief: f32,
        human_belief: f32,
    ) -> Self {
        Self {
            dcs: DcsEngine::with_thresholds(verify_threshold, validate_threshold),
            policy: DeployPolicy::new(live_belief, human_belief),
        }
    }

    /// Run the full pipeline over every tile in `tiles`.
    pub fn run(
        &self,
        problem_desc: impl Into<String>,
        tiles: Vec<TileInput>,
        agents: Vec<AgentInput>,
    ) -> E2EResult {
        let start = now_ns();
        let tile_results: Vec<TileResult> = tiles
            .iter()
            .map(|tile| self.process_tile(tile, &agents))
            .collect();
        let processing_time_ns = now_ns().saturating_sub(start);
        let stats = compute_stats(&tile_results, processing_time_ns);
        E2EResult { problem_desc: problem_desc.into(), tile_results, stats }
    }

    /// Run one DCS cycle for `tile` and convert to `TileResult`.
    fn process_tile(&self, tile: &TileInput, agents: &[AgentInput]) -> TileResult {
        let dcs_tile = DcsTile::new(tile.content.clone(), tile.domain.clone(), tile.complexity);
        let dcs_agents: Vec<DcsAgent> = agents
            .iter()
            .map(|a| DcsAgent::new(a.name.clone(), a.specialty.clone(), a.trust_score))
            .collect();

        let state = self.dcs.run(DcsState::new(dcs_tile, dcs_agents));

        if state.phase == Phase::Complete {
            let sol = &state.verified_solutions[0];
            let (agent_trust, is_specialist) = agents
                .iter()
                .find(|a| a.name == sol.agent_name)
                .map(|a| (a.trust_score, a.specialty == tile.domain))
                .unwrap_or((0.5, false));

            let belief = self.score_to_belief(sol.score, agent_trust, is_specialist);
            let decision = self.policy.classify(belief.confidence, belief.trust, belief.relevance);
            TileResult {
                tile_content: tile.content.clone(),
                agent_name: sol.agent_name.clone(),
                is_specialist,
                dcs_score: sol.score,
                belief,
                decision,
            }
        } else {
            // DCS failed — minimum belief, mandatory human review.
            let (agent_name, dcs_score, is_specialist) =
                state.assignments.first()
                    .map(|a| {
                        let spec = a.agent.specialty == tile.domain;
                        (a.agent.name.clone(), a.agent.performance_on(&tile.domain), spec)
                    })
                    .unwrap_or_default();
            let belief = BeliefScore::new(0.1, 0.1, 0.1);
            let decision = self.policy.classify(0.1, 0.1, 0.1);
            TileResult { tile_content: tile.content.clone(), agent_name, is_specialist, dcs_score, belief, decision }
        }
    }

    /// Map DCS solution score → three-dimensional BeliefScore.
    ///
    /// ```text
    /// confidence = min(dcs_score / SPECIALIST_RATIO, 1.0)
    ///   specialist (5.88 × trust) / 5.88 = trust      → up to 1.0
    ///   generalist (1.0 × trust)  / 5.88 = trust/5.88 → ~0.17 at trust=1.0
    ///
    /// trust      = agent_trust_score   (direct calibrated track record)
    /// relevance  = 1.0 (specialist) | 0.5 (generalist)
    /// ```
    ///
    /// With equal trust: specialist_confidence / generalist_confidence = 5.88 exactly.
    pub(crate) fn score_to_belief(&self, dcs_score: f64, agent_trust: f64, is_specialist: bool) -> BeliefScore {
        let confidence = (dcs_score / SPECIALIST_RATIO).min(1.0) as f32;
        let trust      = agent_trust as f32;
        let relevance  = if is_specialist { 1.0_f32 } else { 0.5_f32 };
        BeliefScore::new(confidence, trust, relevance)
    }
}

fn compute_stats(results: &[TileResult], processing_time_ns: u64) -> PipelineStats {
    let total    = results.len();
    let verified = results.iter().filter(|r| r.dcs_score > 0.0).count();
    let live     = results.iter().filter(|r| r.decision.tier == Tier::Live).count();
    let monitored    = results.iter().filter(|r| r.decision.tier == Tier::Monitored).count();
    let human_gated  = results.iter().filter(|r| r.decision.tier == Tier::HumanGated).count();

    let avg_composite = if total == 0 { 0.0 }
        else { results.iter().map(|r| r.belief.composite()).sum::<f32>() / total as f32 };

    let avg_of = |v: Vec<f32>| -> f32 {
        if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
    };
    let specialist_avg_confidence = avg_of(
        results.iter().filter(|r| r.is_specialist).map(|r| r.belief.confidence).collect());
    let generalist_avg_confidence = avg_of(
        results.iter().filter(|r| !r.is_specialist).map(|r| r.belief.confidence).collect());

    PipelineStats {
        total_tiles: total, verified_tiles: verified,
        tier_live: live, tier_monitored: monitored, tier_human_gated: human_gated,
        avg_belief_composite: avg_composite,
        specialist_avg_confidence, generalist_avg_confidence,
        processing_time_ns,
    }
}

fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
}

// ═══════════════════════════════════════════════════════════════════════════════
// § 5  INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Shared fixtures ───────────────────────────────────────────────────────

    fn four_specialists() -> Vec<AgentInput> {
        vec![
            AgentInput::new("MathBot",  Domain::Math,     1.0),
            AgentInput::new("LogicBot", Domain::Logic,    1.0),
            AgentInput::new("CodeBot",  Domain::Code,     1.0),
            AgentInput::new("LangBot",  Domain::Language, 1.0),
        ]
    }

    fn four_matched_tiles() -> Vec<TileInput> {
        vec![
            TileInput::new("integrate f(x) dx",  Domain::Math,     0.25),
            TileInput::new("verify predicate P",  Domain::Logic,    0.25),
            TileInput::new("parse JSON schema",   Domain::Code,     0.25),
            TileInput::new("translate paragraph", Domain::Language, 0.25),
        ]
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Scenario 1: Happy path — 4 specialists, all tiles Live
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_happy_path_all_live() {
        let pipeline = E2EPipeline::new();
        let result = pipeline.run(
            "Happy path: 4 specialists, 4 matched tiles",
            four_matched_tiles(),
            four_specialists(),
        );

        assert_eq!(result.stats.total_tiles,    4);
        assert_eq!(result.stats.verified_tiles, 4, "all tiles pass DCS verify");
        assert_eq!(result.stats.tier_live,      4, "all 4 must be Live");
        assert_eq!(result.stats.tier_monitored,    0);
        assert_eq!(result.stats.tier_human_gated,  0);

        for tr in &result.tile_results {
            assert!(tr.is_specialist, "tile '{}' should be specialist-assigned", tr.tile_content);
            assert!((tr.dcs_score - SPECIALIST_RATIO).abs() < 1e-9,
                "specialist dcs_score must be {SPECIALIST_RATIO}, got {}", tr.dcs_score);
            assert_eq!(tr.decision.tier, Tier::Live);
            assert!(tr.decision.is_auto(), "Live tiles must not require human");
            assert!(tr.belief.composite() >= 0.8,
                "specialist composite ≥ 0.8, got {:.4}", tr.belief.composite());
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Scenario 2: Mixed tiers — Live + Monitored + HumanGated
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Agent pool:
    //   MathBot   (Math,    trust=1.0) → Math  tile → composite=1.0   → Live
    //   LogicBot  (Logic,   trust=0.7) → Logic tile → composite≈0.788 → Monitored
    //   CodeBot   (Code,    trust=0.5) → Code  tile → composite≈0.630 → Monitored
    //   Generalist(General, trust=1.0) → Lang  tile → composite≈0.44  → HumanGated
    #[test]
    fn test_mixed_tiers() {
        let pipeline = E2EPipeline::new();
        let agents = vec![
            AgentInput::new("MathBot",    Domain::Math,    1.0),
            AgentInput::new("LogicBot",   Domain::Logic,   0.7),
            AgentInput::new("CodeBot",    Domain::Code,    0.5),
            AgentInput::new("Generalist", Domain::General, 1.0),
        ];
        let tiles = vec![
            TileInput::new("solve integration", Domain::Math,     0.25),
            TileInput::new("verify predicate",  Domain::Logic,    0.25),
            TileInput::new("parse expression",  Domain::Code,     0.25),
            TileInput::new("translate text",    Domain::Language, 0.25), // no Language specialist
        ];

        let result = pipeline.run("Mixed: quality gradient + unmatched domain", tiles, agents);

        assert_eq!(result.stats.total_tiles, 4);

        let math = result.tile_results.iter().find(|r| r.tile_content.contains("integration")).unwrap();
        assert_eq!(math.decision.tier, Tier::Live, "Math(trust=1.0) → Live");
        assert!(math.is_specialist);

        let logic = result.tile_results.iter().find(|r| r.tile_content.contains("predicate")).unwrap();
        assert_eq!(logic.decision.tier, Tier::Monitored, "Logic(trust=0.7) → Monitored");
        assert!(logic.is_specialist);

        let code = result.tile_results.iter().find(|r| r.tile_content.contains("expression")).unwrap();
        assert_eq!(code.decision.tier, Tier::Monitored, "Code(trust=0.5) → Monitored");
        assert!(code.is_specialist);

        let lang = result.tile_results.iter().find(|r| r.tile_content.contains("translate")).unwrap();
        assert_eq!(lang.decision.tier, Tier::HumanGated, "Language (no specialist) → HumanGated");
        assert!(!lang.is_specialist);

        assert!(result.stats.tier_live >= 1);
        assert!(result.stats.tier_monitored >= 1);
        assert!(result.stats.tier_human_gated >= 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Scenario 3: Total failure — all HumanGated
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Agents with trust=0.05: specialist scores 5.88×0.05=0.294 < verify threshold 0.5.
    // Every DCS cycle fails → all tiles get HumanGated.
    #[test]
    fn test_all_agents_fail_all_human_gated() {
        let pipeline = E2EPipeline::new(); // verify_threshold = 0.5
        let weak_agents = vec![
            AgentInput::new("WeakMath",  Domain::Math,  0.05), // 5.88×0.05=0.294 < 0.5 ✗
            AgentInput::new("WeakLogic", Domain::Logic, 0.05), // 5.88×0.05=0.294 < 0.5 ✗
        ];
        let tiles = vec![
            TileInput::new("prove theorem",     Domain::Math,  0.25),
            TileInput::new("check consistency", Domain::Logic, 0.25),
            TileInput::new("refactor module",   Domain::Code,  0.25),
        ];

        let result = pipeline.run("Failure: all below verify threshold", tiles, weak_agents);

        assert_eq!(result.stats.total_tiles,      3);
        assert_eq!(result.stats.tier_human_gated, 3, "all tiles must be HumanGated");
        assert_eq!(result.stats.tier_live,        0);
        assert_eq!(result.stats.tier_monitored,   0);

        for tr in &result.tile_results {
            assert!(tr.decision.requires_human);
            assert_eq!(tr.decision.tier, Tier::HumanGated);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Key assertion: 5.88× specialist ratio visible in belief confidence
    // ─────────────────────────────────────────────────────────────────────────

    /// With trust=1.0 for both cohorts:
    ///   specialist_confidence  = 5.88 / 5.88 = 1.000
    ///   generalist_confidence  = 1.00 / 5.88 = 0.170
    ///   ratio                  = 1.000 / 0.170 = 5.88 ✓
    #[test]
    fn test_specialist_ratio_reflected_in_belief() {
        let pipeline = E2EPipeline::new();
        let agents = vec![
            AgentInput::new("MathBot",      Domain::Math,    1.0),
            AgentInput::new("GeneralistBot", Domain::General, 1.0),
        ];
        let tiles = vec![
            TileInput::new("specialist task",    Domain::Math,     0.25),
            TileInput::new("no-specialist task", Domain::Language, 0.25),
        ];

        let result = pipeline.run("Ratio assertion", tiles, agents);

        let spec = result.tile_results.iter().find(|r| r.is_specialist)
            .expect("must have a specialist tile");
        let gen = result.tile_results.iter().find(|r| !r.is_specialist)
            .expect("must have a non-specialist tile");

        let ratio = spec.belief.confidence / gen.belief.confidence;
        assert!(
            (ratio - SPECIALIST_RATIO as f32).abs() < 0.01,
            "belief confidence ratio must be {SPECIALIST_RATIO}×, got {ratio:.4}×"
        );

        assert_eq!(spec.decision.tier, Tier::Live,        "specialist (composite=1.0) → Live");
        assert_eq!(gen.decision.tier,  Tier::HumanGated,  "generalist (composite≈0.44) → HumanGated");
    }

    #[test]
    fn test_specialist_confidence_ratio_accessor() {
        let pipeline = E2EPipeline::new();
        let result = pipeline.run(
            "accessor test",
            vec![
                TileInput::new("math task",     Domain::Math,     0.25),
                TileInput::new("language task", Domain::Language, 0.25),
            ],
            vec![
                AgentInput::new("MathBot", Domain::Math,    1.0),
                AgentInput::new("GenBot",  Domain::General, 1.0),
            ],
        );
        let ratio = result.specialist_confidence_ratio().expect("both cohorts present");
        assert!(
            (ratio - SPECIALIST_RATIO as f32).abs() < 0.05,
            "accessor should return ≈{SPECIALIST_RATIO}, got {ratio}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Belief score formula unit tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_score_to_belief_specialist() {
        let pipeline = E2EPipeline::new();
        let b = pipeline.score_to_belief(SPECIALIST_RATIO, 1.0, true);
        assert!((b.confidence - 1.0).abs() < 1e-6);
        assert!((b.trust      - 1.0).abs() < 1e-6);
        assert!((b.relevance  - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_score_to_belief_generalist() {
        let pipeline = E2EPipeline::new();
        let b = pipeline.score_to_belief(1.0, 1.0, false);
        let expected = (1.0_f64 / SPECIALIST_RATIO) as f32;
        assert!((b.confidence - expected).abs() < 1e-5,
            "generalist confidence = 1/SPECIALIST_RATIO = {expected:.5}, got {}", b.confidence);
        assert!((b.relevance - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_belief_composite_geometric_mean() {
        assert!((BeliefScore::new(1.0, 1.0, 1.0).composite() - 1.0).abs() < 1e-6);
        assert!((BeliefScore::new(0.0, 1.0, 1.0).composite() - 0.0).abs() < 1e-6,
            "zero in one dimension collapses composite");
        assert!((BeliefScore::new(0.5, 0.5, 0.5).composite() - 0.5).abs() < 1e-5);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Deploy tier boundary checks
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_tier_live_boundary() {
        let p = DeployPolicy::default();
        // composite = (0.9*0.9*0.9)^(1/3) = 0.9 ≥ 0.8 → Live
        assert_eq!(p.classify(0.9, 0.9, 0.9).tier, Tier::Live);
    }

    #[test]
    fn test_tier_monitored_boundary() {
        let p = DeployPolicy::default();
        // composite = (0.7*0.7*1.0)^(1/3) ≈ 0.788 in [0.5, 0.8) → Monitored
        assert_eq!(p.classify(0.7, 0.7, 1.0).tier, Tier::Monitored);
    }

    #[test]
    fn test_tier_human_gated_boundary() {
        let p = DeployPolicy::default();
        assert_eq!(p.classify(0.1, 0.1, 0.1).tier, Tier::HumanGated);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Stats consistency
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_stats_tier_counts_sum_to_total() {
        let pipeline = E2EPipeline::new();
        let result = pipeline.run("stats", four_matched_tiles(), four_specialists());
        assert_eq!(
            result.stats.tier_live + result.stats.tier_monitored + result.stats.tier_human_gated,
            result.stats.total_tiles,
        );
    }

    #[test]
    fn test_stats_avg_composite_in_range() {
        let pipeline = E2EPipeline::new();
        let result = pipeline.run("avg", four_matched_tiles(), four_specialists());
        assert!((0.0..=1.0).contains(&result.stats.avg_belief_composite));
    }

    #[test]
    fn test_live_ratio_full_specialists() {
        let pipeline = E2EPipeline::new();
        let result = pipeline.run("ratio", four_matched_tiles(), four_specialists());
        assert!((result.live_ratio() - 1.0).abs() < 1e-9);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Edge cases
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_empty_tile_set() {
        let result = E2EPipeline::new().run("empty", vec![], four_specialists());
        assert_eq!(result.stats.total_tiles, 0);
        assert!(result.tile_results.is_empty());
    }

    #[test]
    fn test_empty_agent_pool_is_human_gated() {
        let result = E2EPipeline::new().run(
            "no agents",
            vec![TileInput::new("task", Domain::Math, 0.25)],
            vec![],
        );
        assert_eq!(result.stats.tier_human_gated, 1);
    }

    #[test]
    fn test_single_tile_single_specialist_is_live() {
        let result = E2EPipeline::new().run(
            "single",
            vec![TileInput::new("integrate f(x)", Domain::Math, 0.25)],
            vec![AgentInput::new("MathBot", Domain::Math, 1.0)],
        );
        assert_eq!(result.stats.tier_live, 1);
        assert_eq!(result.tile_results[0].decision.tier, Tier::Live);
    }

    #[test]
    fn test_custom_thresholds_promote_monitored_to_live() {
        // Lower live threshold to 0.5: composite≈0.788 (LogicBot trust=0.7) becomes Live.
        let pipeline = E2EPipeline::with_thresholds(0.5, 1.0, 0.5, 0.2);
        let result = pipeline.run(
            "easy thresholds",
            vec![TileInput::new("task", Domain::Math, 0.25)],
            vec![AgentInput::new("MathBot", Domain::Math, 0.7)],
        );
        // composite = (0.7 * 0.7 * 1.0)^(1/3) ≈ 0.788 ≥ 0.5 → Live under relaxed config
        assert_eq!(result.tile_results[0].decision.tier, Tier::Live);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // DCS internals: verify the Oracle1 fleet score from the inlined engine
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_dcs_fleet_score_21_87x() {
        let dcs = DcsEngine::new();
        let tile = DcsTile::new("math problem", Domain::Math, 0.25);
        let agents = vec![DcsAgent::new("MathBot", Domain::Math, 1.0)];
        let result = dcs.run(DcsState::new(tile, agents));

        assert!(result.committed, "DCS must commit");
        assert!(
            (result.validation_score - DCS_FLEET_RATIO).abs() < 1e-9,
            "fleet score must be {DCS_FLEET_RATIO}, got {}", result.validation_score
        );
    }

    #[test]
    fn test_dcs_specialist_ratio_5_88x() {
        let spec = DcsAgent::new("MathBot",    Domain::Math,    1.0);
        let gen  = DcsAgent::new("AllRounder", Domain::General, 1.0);
        let ratio = spec.performance_on(&Domain::Math) / gen.performance_on(&Domain::Math);
        assert!(
            (ratio - SPECIALIST_RATIO).abs() < 1e-12,
            "must be exactly {SPECIALIST_RATIO}×, got {ratio}"
        );
    }
}
