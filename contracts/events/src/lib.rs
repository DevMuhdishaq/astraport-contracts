#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, Env, Map,
    Symbol, Vec, U256,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const OK: Symbol = symbol_short!("OK");
const TRIG_ADD: Symbol = symbol_short!("TRIG_ADD");
const ANALYSIS: Symbol = symbol_short!("ANALYSIS");
const RECOMMEND: Symbol = symbol_short!("RECMD");
const TIMEOUT: Symbol = symbol_short!("TIMEOUT");

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by the events contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyExists = 1,
    NotFound = 2,
    Unauthorized = 3,
    InvalidState = 4,
}

// ---------------------------------------------------------------------------
// Portfolio event types for the event emission / subscription system
// ---------------------------------------------------------------------------

/// Portfolio-specific event types emitted when portfolio state changes.
///
/// Subscribers can filter on these types when subscribing.
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortfolioEventType {
    /// Portfolio was rebalanced.
    Rebalanced = 0,
    /// A balance changed (stake, unstake, deposit, withdrawal).
    BalanceChanged = 1,
    /// The target allocation was updated.
    AllocationUpdated = 2,
    /// A configured threshold was breached.
    ThresholdBreached = 3,
    /// A trade was executed.
    TradeExecuted = 4,
    /// A price alert was triggered.
    PriceAlertTriggered = 5,
    /// Free-form custom event.
    Custom = 99,
}

// ---------------------------------------------------------------------------
// Event record
// ---------------------------------------------------------------------------

/// An immutable event record emitted when portfolio state changes.
///
/// Stored in a per-portfolio vector and also published as a Soroban event so
/// off-chain indexers can pick it up. The `event_id` is a monotonically
/// increasing identifier unique across all portfolios.
#[contracttype]
#[derive(Debug, Clone)]
pub struct Event {
    /// Globally unique, monotonically increasing identifier.
    pub event_id: u64,
    /// The portfolio this event concerns.
    pub portfolio_id: Symbol,
    /// The kind of change that occurred.
    pub event_type: PortfolioEventType,
    /// Ledger timestamp when the event was emitted.
    pub timestamp: u64,
    /// Arbitrary key-value details attached to the event.
    pub details: Map<Symbol, Bytes>,
    /// Opaque metadata blob (empty if none).
    pub metadata: Bytes,
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

/// A subscription registration.
///
/// Each subscription records which portfolio it watches, which event types it
/// cares about (empty = all), and the ordering index so that subscribers are
/// notified in the order they registered.
#[contracttype]
#[derive(Debug, Clone)]
pub struct Subscription {
    /// The subscriber (external contract or account).
    pub subscriber: Address,
    /// The portfolio this subscription watches.
    pub portfolio_id: Symbol,
    /// The event types the subscriber is interested in.
    /// An empty vector means "all event types".
    pub event_types: Vec<PortfolioEventType>,
    /// The ordering index assigned at subscription time (0-based).
    pub order_index: u32,
    /// Ledger timestamp when the subscription was created.
    pub subscribed_at: u64,
    /// Whether the subscription is currently active.
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// Supporting types (AI trigger framework – kept from the original contract)
// ---------------------------------------------------------------------------

/// Supported event types that can trigger AI analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EventType {
    PortfolioRebalance = 0,
    TradeExecuted = 1,
    PriceThresholdCrossed = 2,
    VolatilitySpike = 3,
    LiquidityChange = 4,
    CustomEvent = 99,
}

impl From<u32> for EventType {
    fn from(value: u32) -> Self {
        match value {
            0 => EventType::PortfolioRebalance,
            1 => EventType::TradeExecuted,
            2 => EventType::PriceThresholdCrossed,
            3 => EventType::VolatilitySpike,
            4 => EventType::LiquidityChange,
            _ => EventType::CustomEvent,
        }
    }
}

/// Comparison operators for threshold conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ComparisonOperator {
    GreaterThan = 0,
    LessThan = 1,
    EqualTo = 2,
    GreaterOrEqual = 3,
    LessOrEqual = 4,
}

impl From<u32> for ComparisonOperator {
    fn from(value: u32) -> Self {
        match value {
            0 => ComparisonOperator::GreaterThan,
            1 => ComparisonOperator::LessThan,
            2 => ComparisonOperator::EqualTo,
            3 => ComparisonOperator::GreaterOrEqual,
            4 => ComparisonOperator::LessOrEqual,
            _ => ComparisonOperator::GreaterThan,
        }
    }
}

/// Status of an analysis request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AnalysisStatus {
    Pending = 0,
    InProgress = 1,
    Completed = 2,
    Failed = 3,
    TimedOut = 4,
}

impl From<u32> for AnalysisStatus {
    fn from(value: u32) -> Self {
        match value {
            0 => AnalysisStatus::Pending,
            1 => AnalysisStatus::InProgress,
            2 => AnalysisStatus::Completed,
            3 => AnalysisStatus::Failed,
            4 => AnalysisStatus::TimedOut,
            _ => AnalysisStatus::Pending,
        }
    }
}

/// Recommendation action types from AI analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RecommendationType {
    Hold = 0,
    Buy = 1,
    Sell = 2,
    Rebalance = 3,
    Monitor = 4,
    NoAction = 5,
}

impl From<u32> for RecommendationType {
    fn from(value: u32) -> Self {
        match value {
            0 => RecommendationType::Hold,
            1 => RecommendationType::Buy,
            2 => RecommendationType::Sell,
            3 => RecommendationType::Rebalance,
            4 => RecommendationType::Monitor,
            _ => RecommendationType::NoAction,
        }
    }
}

/// AITrigger defines when AI analysis should be invoked.
#[contracttype]
#[derive(Debug, Clone)]
pub struct AITrigger {
    pub trigger_id: Symbol,
    pub name: Symbol,
    pub event_types: Vec<u32>,
    pub has_threshold: bool,
    pub threshold: U256,
    pub has_operator: bool,
    pub operator: u32,
    pub ai_service_endpoint: Address,
    pub timeout: u64,
    pub is_active: bool,
    pub owner: Address,
}

/// Condition to evaluate against current values.
pub struct TriggerCondition {
    pub current_value: i128,
    pub threshold: i128,
    pub operator: ComparisonOperator,
}

/// TriggerEvaluator handles condition checking for triggers.
pub struct TriggerEvaluator;

impl TriggerEvaluator {
    pub fn evaluate(
        trigger: &AITrigger,
        event_type: EventType,
        current_value: Option<i128>,
    ) -> bool {
        let event_type_matches = trigger.event_types.contains(event_type as u32);
        if !event_type_matches {
            return false;
        }
        if !trigger.has_threshold || !trigger.has_operator {
            return true;
        }
        let Some(value) = current_value else {
            return false;
        };
        let condition = TriggerCondition {
            current_value: value,
            threshold: trigger.threshold.clone(),
            operator: ComparisonOperator::from(trigger.operator),
        };
        Self::evaluate_condition(&condition)
    }

    fn evaluate_condition(condition: &TriggerCondition) -> bool {
        match condition.operator {
            ComparisonOperator::GreaterThan => condition.current_value > condition.threshold,
            ComparisonOperator::LessThan => condition.current_value < condition.threshold,
            ComparisonOperator::EqualTo => condition.current_value == condition.threshold,
            ComparisonOperator::GreaterOrEqual => condition.current_value >= condition.threshold,
            ComparisonOperator::LessOrEqual => condition.current_value <= condition.threshold,
        }
    }
}

/// AnalysisResult stores the output from AI service analysis.
#[contracttype]
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub analysis_id: u64,
    pub trigger_id: Symbol,
    pub portfolio_id: Symbol,
    pub timestamp: u64,
    pub latency_ms: u64,
    pub status: u32,
    pub raw_output: Bytes,
    pub error_message: Symbol,
}

/// Recommendation generated from AI analysis output.
#[contracttype]
#[derive(Debug, Clone)]
pub struct Recommendation {
    pub recommendation_id: u64,
    pub analysis_id: u64,
    pub portfolio_id: Symbol,
    pub action_type: u32,
    pub asset: Symbol,
    pub has_amount: bool,
    pub amount: U256,
    pub confidence_score: u32,
    pub timestamp: u64,
    pub accepted: Option<bool>,
}

/// AnalysisMetrics tracks performance metrics for AI analysis.
#[contracttype]
#[derive(Debug, Clone, Default)]
pub struct AnalysisMetrics {
    pub total_analyses: u64,
    pub successful_analyses: u64,
    pub failed_analyses: u64,
    pub timed_out_analyses: u64,
    pub average_latency_ms: u64,
    pub recommendations_accepted: u64,
    pub recommendations_rejected: u64,
}

// ---------------------------------------------------------------------------
// AI service client (stub)
// ---------------------------------------------------------------------------

pub trait AIServiceClient {
    fn submit_analysis(
        env: &Env,
        trigger: &AITrigger,
        portfolio_id: Symbol,
        event_data: Bytes,
    ) -> Result<u64, Symbol>;

    fn check_analysis_status(env: &Env, analysis_id: u64) -> Result<AnalysisStatus, Symbol>;
}

pub struct SorobanAIServiceClient;

impl AIServiceClient for SorobanAIServiceClient {
    fn submit_analysis(
        env: &Env,
        trigger: &AITrigger,
        portfolio_id: Symbol,
        _event_data: Bytes,
    ) -> Result<u64, Symbol> {
        let analysis_id = next_id(env);
        env.events().publish(
            (
                symbol_short!("ANAL_SUB"),
                portfolio_id,
                trigger.trigger_id.clone(),
            ),
            analysis_id,
        );
        Ok(analysis_id)
    }

    fn check_analysis_status(env: &Env, analysis_id: u64) -> Result<AnalysisStatus, Symbol> {
        let key = storage_keys::analysis_status(analysis_id);
        if env.storage().persistent().has(&key) {
            let status: u32 = env.storage().persistent().get(&key).unwrap();
            Ok(AnalysisStatus::from(status))
        } else {
            Err(symbol_short!("NOT_FOUND"))
        }
    }
}

pub struct RecommendationEngine;

impl RecommendationEngine {
    pub fn generate_recommendation(
        env: &Env,
        analysis: &AnalysisResult,
        ai_output: &Map<Symbol, u32>,
    ) -> Result<Recommendation, Symbol> {
        if analysis.status != AnalysisStatus::Completed as u32 {
            return Err(symbol_short!("BAD_STATE"));
        }
        let action_type = if let Some(action) = ai_output.get(symbol_short!("action")) {
            match action {
                0 => RecommendationType::Hold as u32,
                1 => RecommendationType::Buy as u32,
                2 => RecommendationType::Sell as u32,
                3 => RecommendationType::Rebalance as u32,
                4 => RecommendationType::Monitor as u32,
                _ => RecommendationType::NoAction as u32,
            }
        } else {
            RecommendationType::NoAction as u32
        };
        let confidence = ai_output.get(symbol_short!("conf")).unwrap_or(0);
        let timestamp = env.ledger().timestamp();
        Ok(Recommendation {
            recommendation_id: next_id(env),
            analysis_id: analysis.analysis_id,
            portfolio_id: analysis.portfolio_id.clone(),
            action_type,
            asset: symbol_short!(""),
            has_amount: false,
            amount: U256::from_u32(env, 0),
            confidence_score: confidence,
            timestamp,
            accepted: None,
        })
    }
}

// ---------------------------------------------------------------------------
// ID generator
// ---------------------------------------------------------------------------

fn next_id(env: &Env) -> u64 {
    let key = symbol_short!("next_id");
    let current: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    let next = current + 1;
    env.storage().persistent().set(&key, &next);
    next
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

mod storage_keys {
    use super::*;

    pub fn triggers() -> Symbol { symbol_short!("triggers") }
    pub fn analyses() -> Symbol { symbol_short!("analyses") }
    pub fn recommendations() -> Symbol { symbol_short!("recs") }
    pub fn metrics() -> Symbol { symbol_short!("metrics") }
    #[allow(dead_code)]
    pub fn subscribers(portfolio_id: Symbol) -> (Symbol, Symbol) { (symbol_short!("subs"), portfolio_id) }
    #[allow(dead_code)]
    pub fn analysis_status(analysis_id: u64) -> (Symbol, u64) { (symbol_short!("status"), analysis_id) }
    pub fn subscriptions(portfolio_id: Symbol) -> (Symbol, Symbol) { (symbol_short!("sub_list"), portfolio_id) }
    pub fn event_history(portfolio_id: Symbol) -> (Symbol, Symbol) { (symbol_short!("ev_hist"), portfolio_id) }
}

// ---------------------------------------------------------------------------
// Event emission helpers
// ---------------------------------------------------------------------------

fn matches_filter(sub: &Subscription, event_type: &PortfolioEventType) -> bool {
    if sub.event_types.len() == 0 {
        return true;
    }
    for i in 0..sub.event_types.len() {
        if sub.event_types.get(i).unwrap() == *event_type {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct EventsContract;

#[contractimpl]
impl EventsContract {
    pub fn initialize(env: Env) -> Symbol {
        if !env.storage().persistent().has(&storage_keys::metrics()) {
            let metrics = AnalysisMetrics::default();
            env.storage().persistent().set(&storage_keys::metrics(), &metrics);
        }
        OK
    }

    pub fn add_trigger(env: Env, trigger: AITrigger) -> Result<Symbol, Error> {
        trigger.owner.require_auth();
        let mut triggers: Map<Symbol, AITrigger> = env.storage().persistent().get(&storage_keys::triggers()).unwrap_or_else(|| Map::new(&env));
        let trigger_id = trigger.trigger_id.clone();
        if triggers.contains_key(trigger_id.clone()) { return Err(Error::AlreadyExists); }
        triggers.set(trigger_id.clone(), trigger);
        env.storage().persistent().set(&storage_keys::triggers(), &triggers);
        env.events().publish((TRIG_ADD,), trigger_id);
        Ok(OK)
    }

    pub fn remove_trigger(env: Env, trigger_id: Symbol, owner: Address) -> Result<Symbol, Error> {
        owner.require_auth();
        let mut triggers: Map<Symbol, AITrigger> = env.storage().persistent().get(&storage_keys::triggers()).ok_or(Error::NotFound)?;
        let trigger = triggers.get(trigger_id.clone()).ok_or(Error::NotFound)?;
        if trigger.owner != owner { return Err(Error::Unauthorized); }
        triggers.remove(trigger_id.clone());
        env.storage().persistent().set(&storage_keys::triggers(), &triggers);
        env.events().publish((symbol_short!("TRIG_RMV"),), trigger_id);
        Ok(OK)
    }

    pub fn process_event(env: Env, portfolio_id: Symbol, event_type: u32, event_data: Bytes, current_value: Option<U256>) -> Result<Vec<u64>, Error> {
        let event = EventType::from(event_type);
        let triggers: Map<Symbol, AITrigger> = env.storage().persistent().get(&storage_keys::triggers()).unwrap_or_else(|| Map::new(&env));
        let mut triggered_analyses = Vec::new(&env);
        let mut metrics: AnalysisMetrics = env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();

        for (trigger_id, trigger) in triggers.iter() {
            if !trigger.is_active { continue; }
            if TriggerEvaluator::evaluate(&trigger, event, current_value.clone()) {
                match SorobanAIServiceClient::submit_analysis(&env, &trigger, portfolio_id.clone(), event_data.clone()) {
                    Ok(analysis_id) => {
                        let mut analyses: Map<u64, AnalysisResult> = env.storage().persistent().get(&storage_keys::analyses()).unwrap_or_else(|| Map::new(&env));
                        let timestamp = env.ledger().timestamp();
                        let analysis = AnalysisResult { analysis_id, trigger_id: trigger_id.clone(), portfolio_id: portfolio_id.clone(), timestamp, latency_ms: 0, status: AnalysisStatus::Pending as u32, raw_output: Bytes::new(&env), error_message: symbol_short!("") };
                        analyses.set(analysis_id, analysis);
                        env.storage().persistent().set(&storage_keys::analyses(), &analyses);
                        env.storage().persistent().set(&storage_keys::analysis_status(analysis_id), &(AnalysisStatus::Pending as u32));
                        metrics.total_analyses += 1;
                        triggered_analyses.push_back(analysis_id);
                        env.events().publish((ANALYSIS, portfolio_id.clone(), trigger_id.clone()), analysis_id);
                    }
                    Err(e) => { env.events().publish((symbol_short!("ERROR"), trigger_id.clone()), e); }
                }
            }
        }
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);
        Ok(triggered_analyses)
    }

    pub fn update_analysis_status(env: Env, analysis_id: u64, status: u32, latency_ms: Option<u64>, raw_output: Option<Bytes>, error: Option<Symbol>) -> Result<Symbol, Error> {
        let mut analyses: Map<u64, AnalysisResult> = env.storage().persistent().get(&storage_keys::analyses()).ok_or(Error::NotFound)?;
        let mut analysis = analyses.get(analysis_id).ok_or(Error::NotFound)?;
        let new_status = AnalysisStatus::from(status);
        analysis.status = status;
        if let Some(latency) = latency_ms { analysis.latency_ms = latency; }
        if let Some(output) = raw_output { analysis.raw_output = output; }
        analysis.error_message = error.unwrap_or(symbol_short!(""));

        analyses.set(analysis_id, analysis.clone());
        env.storage().persistent().set(&storage_keys::analyses(), &analyses);
        env.storage().persistent().set(&storage_keys::analysis_status(analysis_id), &status);

        let mut metrics: AnalysisMetrics = env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();
        match new_status {
            AnalysisStatus::Completed => {
                metrics.successful_analyses += 1;
                if analysis.latency_ms > 0 && metrics.successful_analyses > 1 {
                    metrics.average_latency_ms = (metrics.average_latency_ms * (metrics.successful_analyses - 1) + analysis.latency_ms) / metrics.successful_analyses;
                } else if analysis.latency_ms > 0 {
                    metrics.average_latency_ms = analysis.latency_ms;
                }
            }
            AnalysisStatus::Failed => metrics.failed_analyses += 1,
            AnalysisStatus::TimedOut => metrics.timed_out_analyses += 1,
            _ => {}
        }
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);

        if new_status == AnalysisStatus::Completed {
            let mut ai_output: Map<Symbol, u32> = Map::new(&env);
            if analysis.raw_output.len() > 0 {
                ai_output.set(symbol_short!("action"), analysis.raw_output.get(0).unwrap_or(0) as u32);
                ai_output.set(symbol_short!("conf"), 85);
            }
            if let Ok(recommendation) = RecommendationEngine::generate_recommendation(&env, &analysis, &ai_output) {
                let mut recommendations: Map<u64, Recommendation> = env.storage().persistent().get(&storage_keys::recommendations()).unwrap_or_else(|| Map::new(&env));
                let rec_id = recommendation.recommendation_id;
                recommendations.set(rec_id, recommendation);
                env.storage().persistent().set(&storage_keys::recommendations(), &recommendations);
                env.events().publish((RECOMMEND, analysis.portfolio_id, analysis_id), rec_id);
            }
        }
        Ok(OK)
    }

    pub fn process_timeout(env: Env, analysis_id: u64) -> Result<Symbol, Error> {
        let mut analyses: Map<u64, AnalysisResult> = env.storage().persistent().get(&storage_keys::analyses()).ok_or(Error::NotFound)?;
        let mut analysis = analyses.get(analysis_id).ok_or(Error::NotFound)?;
        if analysis.status != AnalysisStatus::Pending as u32 && analysis.status != AnalysisStatus::InProgress as u32 { return Err(Error::InvalidState); }
        analysis.status = AnalysisStatus::TimedOut as u32;
        analysis.error_message = TIMEOUT;
        analyses.set(analysis_id, analysis);
        env.storage().persistent().set(&storage_keys::analyses(), &analyses);
        env.storage().persistent().set(&storage_keys::analysis_status(analysis_id), &(AnalysisStatus::TimedOut as u32));
        let mut metrics: AnalysisMetrics = env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();
        metrics.timed_out_analyses += 1;
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);
        env.events().publish((TIMEOUT,), analysis_id);
        Ok(OK)
    }

    pub fn process_recommendation_feedback(env: Env, recommendation_id: u64, accepted: bool, responder: Address) -> Result<Symbol, Error> {
        responder.require_auth();
        let mut recommendations: Map<u64, Recommendation> = env.storage().persistent().get(&storage_keys::recommendations()).ok_or(Error::NotFound)?;
        let mut rec = recommendations.get(recommendation_id).ok_or(Error::NotFound)?;
        rec.accepted = Some(accepted);
        recommendations.set(recommendation_id, rec);
        env.storage().persistent().set(&storage_keys::recommendations(), &recommendations);
        let mut metrics: AnalysisMetrics = env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();
        if accepted { metrics.recommendations_accepted += 1; } else { metrics.recommendations_rejected += 1; }
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);
        Ok(OK)
    }

    pub fn get_portfolio_analyses(env: Env, portfolio_id: Symbol) -> Vec<AnalysisResult> {
        let analyses: Map<u64, AnalysisResult> = env.storage().persistent().get(&storage_keys::analyses()).unwrap_or_else(|| Map::new(&env));
        let mut results = Vec::new(&env);
        for (_, analysis) in analyses.iter() { if analysis.portfolio_id == portfolio_id { results.push_back(analysis); } }
        results
    }

    pub fn get_portfolio_recommendations(env: Env, portfolio_id: Symbol) -> Vec<Recommendation> {
        let recommendations: Map<u64, Recommendation> = env.storage().persistent().get(&storage_keys::recommendations()).unwrap_or_else(|| Map::new(&env));
        let mut results = Vec::new(&env);
        for (_, rec) in recommendations.iter() { if rec.portfolio_id == portfolio_id { results.push_back(rec); } }
        results
    }

    pub fn get_metrics(env: Env) -> AnalysisMetrics {
        env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default()
    }

    pub fn get_all_triggers(env: Env) -> Vec<AITrigger> {
        let triggers: Map<Symbol, AITrigger> = env.storage().persistent().get(&storage_keys::triggers()).unwrap_or_else(|| Map::new(&env));
        let mut results = Vec::new(&env);
        for (_, trigger) in triggers.iter() { results.push_back(trigger); }
        results
    }

    // ===================================================================
    // Event Emission & Subscription System
    // ===================================================================

    /// Subscribe to portfolio events with optional type filtering.
    pub fn subscribe(env: Env, portfolio_id: Symbol, subscriber: Address, event_types: Vec<PortfolioEventType>) -> Result<Symbol, Error> {
        subscriber.require_auth();
        let subs_key = storage_keys::subscriptions(portfolio_id.clone());
        let mut subscriptions: Vec<Subscription> = env.storage().persistent().get(&subs_key).unwrap_or_else(|| Vec::new(&env));
        for i in 0..subscriptions.len() {
            let existing = subscriptions.get(i).unwrap();
            if existing.subscriber == subscriber && existing.is_active { return Ok(OK); }
        }
        let order_index = subscriptions.len();
        let sub = Subscription { subscriber: subscriber.clone(), portfolio_id: portfolio_id.clone(), event_types, order_index, subscribed_at: env.ledger().timestamp(), is_active: true };
        subscriptions.push_back(sub);
        env.storage().persistent().set(&subs_key, &subscriptions);
        env.events().publish((symbol_short!("SUB_ADD"), portfolio_id, subscriber), order_index);
        Ok(OK)
    }

    /// Unsubscribe from portfolio events.
    pub fn unsubscribe(env: Env, portfolio_id: Symbol, subscriber: Address) -> Result<Symbol, Error> {
        subscriber.require_auth();
        let subs_key = storage_keys::subscriptions(portfolio_id.clone());
        let subscriptions: Vec<Subscription> = env.storage().persistent().get(&subs_key).ok_or(Error::NotFound)?;
        let mut found = false;
        let mut updated: Vec<Subscription> = Vec::new(&env);
        for i in 0..subscriptions.len() {
            let mut sub = subscriptions.get(i).unwrap();
            if sub.subscriber == subscriber && sub.is_active { sub.is_active = false; found = true; }
            updated.push_back(sub);
        }
        if !found { return Err(Error::NotFound); }
        env.storage().persistent().set(&subs_key, &updated);
        env.events().publish((symbol_short!("SUB_RMV"), portfolio_id, subscriber), 0u32);
        Ok(OK)
    }

    /// Emit a portfolio event and notify all matching active subscribers.
    pub fn emit_event(env: Env, portfolio_id: Symbol, event_type: PortfolioEventType, details: Map<Symbol, Bytes>, metadata: Bytes) -> Result<Event, Error> {
        let event_id = next_id(&env);
        let timestamp = env.ledger().timestamp();
        let event = Event { event_id, portfolio_id: portfolio_id.clone(), event_type, timestamp, details: details.clone(), metadata };

        let hist_key = storage_keys::event_history(portfolio_id.clone());
        let mut history: Vec<Event> = env.storage().persistent().get(&hist_key).unwrap_or_else(|| Vec::new(&env));
        history.push_back(event.clone());
        env.storage().persistent().set(&hist_key, &history);

        env.events().publish((symbol_short!("PF_EVENT"), portfolio_id.clone(), event_type), event.clone());

        let subs_key = storage_keys::subscriptions(portfolio_id.clone());
        let subscriptions: Vec<Subscription> = env.storage().persistent().get(&subs_key).unwrap_or_else(|| Vec::new(&env));
        for i in 0..subscriptions.len() {
            let sub = subscriptions.get(i).unwrap();
            if sub.is_active && matches_filter(&sub, &event_type) {
                env.events().publish((symbol_short!("NOTIFY"), sub.subscriber.clone(), portfolio_id.clone()), event.event_id);
            }
        }
        Ok(event)
    }

    /// Emit a portfolio event without metadata (convenience).
    pub fn emit_event_simple(env: Env, portfolio_id: Symbol, event_type: PortfolioEventType, details: Map<Symbol, Bytes>) -> Result<Event, Error> {
        Self::emit_event(env.clone(), portfolio_id, event_type, details, Bytes::new(&env))
    }

    /// Subscribe to *all* events (legacy convenience).
    pub fn subscribe_all(env: Env, portfolio_id: Symbol, subscriber: Address) -> Result<Symbol, Error> {
        Self::subscribe(env.clone(), portfolio_id, subscriber, Vec::new(&env))
    }

    /// Unsubscribe from *all* events (legacy convenience).
    pub fn unsubscribe_all(env: Env, portfolio_id: Symbol, subscriber: Address) -> Result<Symbol, Error> {
        Self::unsubscribe(env, portfolio_id, subscriber)
    }

    // --- Query functions ---

    pub fn get_subscriptions(env: Env, portfolio_id: Symbol) -> Vec<Subscription> {
        let subs_key = storage_keys::subscriptions(portfolio_id);
        env.storage().persistent().get(&subs_key).unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_active_subscriptions(env: Env, portfolio_id: Symbol) -> Vec<Subscription> {
        let all = Self::get_subscriptions(env.clone(), portfolio_id);
        let mut active = Vec::new(&env);
        for i in 0..all.len() { let sub = all.get(i).unwrap(); if sub.is_active { active.push_back(sub); } }
        active
    }

    pub fn get_event_history(env: Env, portfolio_id: Symbol) -> Vec<Event> {
        let hist_key = storage_keys::event_history(portfolio_id);
        env.storage().persistent().get(&hist_key).unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_events_by_type(env: Env, portfolio_id: Symbol, event_type: PortfolioEventType) -> Vec<Event> {
        let all = Self::get_event_history(env.clone(), portfolio_id.clone());
        let mut filtered = Vec::new(&env);
        for i in 0..all.len() { let event = all.get(i).unwrap(); if event.event_type == event_type { filtered.push_back(event); } }
        filtered
    }

    pub fn get_events_by_time_range(env: Env, portfolio_id: Symbol, from_timestamp: u64, to_timestamp: u64) -> Vec<Event> {
        let all = Self::get_event_history(env.clone(), portfolio_id.clone());
        let mut filtered = Vec::new(&env);
        for i in 0..all.len() { let event = all.get(i).unwrap(); if event.timestamp >= from_timestamp && event.timestamp <= to_timestamp { filtered.push_back(event); } }
        filtered
    }

    pub fn get_events_filtered(env: Env, portfolio_id: Symbol, event_type: PortfolioEventType, from_timestamp: u64, to_timestamp: u64) -> Vec<Event> {
        let all = Self::get_event_history(env.clone(), portfolio_id.clone());
        let mut filtered = Vec::new(&env);
        for i in 0..all.len() { let event = all.get(i).unwrap(); if event.event_type == event_type && event.timestamp >= from_timestamp && event.timestamp <= to_timestamp { filtered.push_back(event); } }
        filtered
    }

    pub fn get_event_count(env: Env, portfolio_id: Symbol) -> u32 {
        let hist_key = storage_keys::event_history(portfolio_id);
        let history: Vec<Event> = env.storage().persistent().get(&hist_key).unwrap_or_else(|| Vec::new(&env));
        history.len()
    }
}

// Tests are in contracts/events/tests/integration_tests.rs
