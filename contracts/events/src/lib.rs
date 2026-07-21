#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, symbol_short, Address, Bytes, Env, Map, Symbol, Vec,
    U256,
};

// Symbols used as fixed return / event values throughout the contract.
const OK: Symbol = symbol_short!("OK");
const TRIG_ADD: Symbol = symbol_short!("TRIG_ADD");
const ANALYSIS: Symbol = symbol_short!("ANALYSIS");
const RECOMMEND: Symbol = symbol_short!("RECMD");
const TIMEOUT: Symbol = symbol_short!("TIMEOUT");

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

/// Supported event types that can trigger AI analysis
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

/// Comparison operators for threshold conditions
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

/// Status of an analysis request
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

/// Recommendation action types from AI analysis
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

/// AITrigger defines when AI analysis should be invoked
#[soroban_sdk::contracttype]
#[derive(Debug, Clone)]
pub struct AITrigger {
    pub trigger_id: Symbol,
    pub name: Symbol,
    pub event_types: Vec<u32>, // Vec of EventType as u32
    pub threshold: Option<U256>,
    pub operator: Option<u32>, // ComparisonOperator as u32
    pub ai_service_endpoint: Address,
    pub timeout: u64, // Timeout in milliseconds
    pub is_active: bool,
    pub owner: Address,
}

/// Condition to evaluate against current values
pub struct TriggerCondition {
    pub current_value: U256,
    pub threshold: U256,
    pub operator: ComparisonOperator,
}

/// TriggerEvaluator handles condition checking for triggers
pub struct TriggerEvaluator;

impl TriggerEvaluator {
    /// Evaluate if a trigger's conditions are met
    pub fn evaluate(trigger: &AITrigger, event_type: EventType, current_value: Option<U256>) -> bool {
        // First check if this event type is in the trigger's supported events
        let event_type_matches = trigger.event_types.contains(event_type as u32);
        if !event_type_matches {
            return false;
        }

        // If no threshold is defined, trigger always fires for matching event types
        if trigger.threshold.is_none() || trigger.operator.is_none() {
            return true;
        }

        // If we have a threshold but no current value provided, can't evaluate
        let Some(value) = current_value else {
            return false;
        };

        // Evaluate the threshold condition
        let condition = TriggerCondition {
            current_value: value,
            threshold: trigger.threshold.clone().unwrap(),
            operator: ComparisonOperator::from(trigger.operator.unwrap()),
        };

        Self::evaluate_condition(&condition)
    }

    /// Evaluate a specific condition
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

/// AnalysisResult stores the output from AI service analysis
#[soroban_sdk::contracttype]
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub analysis_id: u64,
    pub trigger_id: Symbol,
    pub portfolio_id: Symbol,
    pub timestamp: u64,
    pub latency_ms: u64,
    pub status: u32, // AnalysisStatus as u32
    pub raw_output: Bytes,
    pub error_message: Option<Symbol>,
}

/// Recommendation generated from AI analysis output
#[soroban_sdk::contracttype]
#[derive(Debug, Clone)]
pub struct Recommendation {
    pub recommendation_id: u64,
    pub analysis_id: u64,
    pub portfolio_id: Symbol,
    pub action_type: u32, // RecommendationType as u32
    pub asset: Option<Symbol>,
    pub amount: Option<U256>,
    pub confidence_score: u32, // 0-100
    pub timestamp: u64,
    pub accepted: Option<bool>,
}

/// AnalysisMetrics tracks performance metrics for AI analysis
#[soroban_sdk::contracttype]
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

/// AIServiceClient interface for external AI service communication
pub trait AIServiceClient {
    /// Submit analysis request to external AI service
    fn submit_analysis(
        env: &Env,
        trigger: &AITrigger,
        portfolio_id: Symbol,
        event_data: Bytes,
    ) -> Result<u64, Symbol>;

    /// Check the status of a submitted analysis
    fn check_analysis_status(env: &Env, analysis_id: u64) -> Result<AnalysisStatus, Symbol>;
}

/// Default implementation of AIServiceClient for Soroban environment
pub struct SorobanAIServiceClient;

impl AIServiceClient for SorobanAIServiceClient {
    fn submit_analysis(
        env: &Env,
        trigger: &AITrigger,
        portfolio_id: Symbol,
        _event_data: Bytes,
    ) -> Result<u64, Symbol> {
        // In a real implementation, this would make a cross-contract call to the AI service.
        // For this framework, we allocate a unique analysis ID and record the request.
        let analysis_id = next_id(env);

        // Emit event that analysis was submitted
        env.events().publish(
            (symbol_short!("ANAL_SUB"), portfolio_id, trigger.trigger_id.clone()),
            analysis_id,
        );

        Ok(analysis_id)
    }

    fn check_analysis_status(env: &Env, analysis_id: u64) -> Result<AnalysisStatus, Symbol> {
        // In a real implementation, this would query the AI service contract.
        // For this framework, we simulate status checking from stored state.
        let key = storage_keys::analysis_status(analysis_id);
        if env.storage().persistent().has(&key) {
            let status: u32 = env.storage().persistent().get(&key).unwrap();
            Ok(AnalysisStatus::from(status))
        } else {
            Err(symbol_short!("NOT_FOUND"))
        }
    }
}

/// RecommendationEngine converts AI output to actionable recommendations
pub struct RecommendationEngine;

impl RecommendationEngine {
    /// Generate a recommendation from analysis results
    pub fn generate_recommendation(
        env: &Env,
        analysis: &AnalysisResult,
        ai_output: &Map<Symbol, u32>,
    ) -> Result<Recommendation, Symbol> {
        if analysis.status != AnalysisStatus::Completed as u32 {
            return Err(symbol_short!("BAD_STATE"));
        }

        // Parse AI output to determine recommendation
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
            asset: None,
            amount: None,
            confidence_score: confidence,
            timestamp,
            accepted: None,
        })
    }
}

/// Allocate a unique, monotonically increasing identifier.
fn next_id(env: &Env) -> u64 {
    let key = symbol_short!("next_id");
    let current: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    let next = current + 1;
    env.storage().persistent().set(&key, &next);
    next
}

/// Storage keys for persistent data
mod storage_keys {
    use super::*;

    pub fn triggers() -> Symbol {
        symbol_short!("triggers")
    }

    pub fn analyses() -> Symbol {
        symbol_short!("analyses")
    }

    pub fn recommendations() -> Symbol {
        symbol_short!("recs")
    }

    pub fn metrics() -> Symbol {
        symbol_short!("metrics")
    }

    pub fn subscribers(portfolio_id: Symbol) -> (Symbol, Symbol) {
        (symbol_short!("subs"), portfolio_id)
    }

    pub fn analysis_status(analysis_id: u64) -> (Symbol, u64) {
        (symbol_short!("status"), analysis_id)
    }
}

/// Events contract for AstraPort
/// Implements AI Analysis Trigger Framework for Market Events
#[contract]
pub struct EventsContract;

#[contractimpl]
impl EventsContract {
    /// Initialize the events contract
    pub fn initialize(env: Env) -> Symbol {
        // Initialize metrics if not already set
        if !env.storage().persistent().has(&storage_keys::metrics()) {
            let metrics = AnalysisMetrics::default();
            env.storage().persistent().set(&storage_keys::metrics(), &metrics);
        }
        OK
    }

    /// Add a new AI trigger to the system
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `trigger` - The AITrigger configuration to add
    ///
    /// # Returns
    /// Success symbol if trigger was added
    pub fn add_trigger(env: Env, trigger: AITrigger) -> Result<Symbol, Error> {
        // Verify caller owns the trigger
        trigger.owner.require_auth();

        // Store triggers in a map keyed by trigger_id
        let mut triggers: Map<Symbol, AITrigger> = env
            .storage()
            .persistent()
            .get(&storage_keys::triggers())
            .unwrap_or_else(|| Map::new(&env));

        let trigger_id = trigger.trigger_id.clone();

        if triggers.contains_key(trigger_id.clone()) {
            return Err(Error::AlreadyExists);
        }

        triggers.set(trigger_id.clone(), trigger);
        env.storage().persistent().set(&storage_keys::triggers(), &triggers);

        // Emit trigger added event
        env.events().publish((TRIG_ADD,), trigger_id);

        Ok(OK)
    }

    /// Remove an existing AI trigger
    pub fn remove_trigger(env: Env, trigger_id: Symbol, owner: Address) -> Result<Symbol, Error> {
        owner.require_auth();

        let mut triggers: Map<Symbol, AITrigger> = env
            .storage()
            .persistent()
            .get(&storage_keys::triggers())
            .ok_or(Error::NotFound)?;

        let trigger = triggers.get(trigger_id.clone()).ok_or(Error::NotFound)?;

        if trigger.owner != owner {
            return Err(Error::Unauthorized);
        }

        triggers.remove(trigger_id.clone());
        env.storage().persistent().set(&storage_keys::triggers(), &triggers);

        env.events().publish((symbol_short!("TRIG_RMV"),), trigger_id);

        Ok(OK)
    }

    /// Process an event, evaluate triggers, and invoke AI analysis if conditions are met
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `portfolio_id` - Portfolio identifier
    /// * `event_type` - Type of event that occurred
    /// * `event_data` - Additional event data
    /// * `current_value` - Optional current value for threshold evaluation
    ///
    /// # Returns
    /// Vector of analysis IDs that were triggered
    pub fn process_event(
        env: Env,
        portfolio_id: Symbol,
        event_type: u32,
        event_data: Bytes,
        current_value: Option<U256>,
    ) -> Result<Vec<u64>, Error> {
        let event = EventType::from(event_type);
        let triggers: Map<Symbol, AITrigger> = env
            .storage()
            .persistent()
            .get(&storage_keys::triggers())
            .unwrap_or_else(|| Map::new(&env));

        let mut triggered_analyses = Vec::new(&env);
        let mut metrics: AnalysisMetrics =
            env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();

        // Check each active trigger
        for (trigger_id, trigger) in triggers.iter() {
            if !trigger.is_active {
                continue;
            }

            if TriggerEvaluator::evaluate(&trigger, event, current_value.clone()) {
                // Trigger conditions met - submit to AI service
                match SorobanAIServiceClient::submit_analysis(
                    &env,
                    &trigger,
                    portfolio_id.clone(),
                    event_data.clone(),
                ) {
                    Ok(analysis_id) => {
                        // Record the analysis
                        let mut analyses: Map<u64, AnalysisResult> = env
                            .storage()
                            .persistent()
                            .get(&storage_keys::analyses())
                            .unwrap_or_else(|| Map::new(&env));

                        let timestamp = env.ledger().timestamp();
                        let analysis = AnalysisResult {
                            analysis_id,
                            trigger_id: trigger_id.clone(),
                            portfolio_id: portfolio_id.clone(),
                            timestamp,
                            latency_ms: 0,
                            status: AnalysisStatus::Pending as u32,
                            raw_output: Bytes::new(&env),
                            error_message: None,
                        };

                        analyses.set(analysis_id, analysis);
                        env.storage().persistent().set(&storage_keys::analyses(), &analyses);

                        // Store status for tracking
                        env.storage()
                            .persistent()
                            .set(&storage_keys::analysis_status(analysis_id), &(AnalysisStatus::Pending as u32));

                        // Update metrics
                        metrics.total_analyses += 1;

                        triggered_analyses.push_back(analysis_id);

                        // Emit analysis triggered event
                        env.events().publish((ANALYSIS, portfolio_id.clone(), trigger_id.clone()), analysis_id);
                    }
                    Err(e) => {
                        // Log the error but continue processing other triggers
                        env.events().publish((symbol_short!("ERROR"), trigger_id.clone()), e);
                    }
                }
            }
        }

        // Save updated metrics
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);

        Ok(triggered_analyses)
    }

    /// Update the status of an analysis
    pub fn update_analysis_status(
        env: Env,
        analysis_id: u64,
        status: u32,
        latency_ms: Option<u64>,
        raw_output: Option<Bytes>,
        error: Option<Symbol>,
    ) -> Result<Symbol, Error> {
        let mut analyses: Map<u64, AnalysisResult> = env
            .storage()
            .persistent()
            .get(&storage_keys::analyses())
            .ok_or(Error::NotFound)?;

        let mut analysis = analyses.get(analysis_id).ok_or(Error::NotFound)?;

        let new_status = AnalysisStatus::from(status);
        analysis.status = status;

        if let Some(latency) = latency_ms {
            analysis.latency_ms = latency;
        }

        if let Some(output) = raw_output {
            analysis.raw_output = output;
        }

        analysis.error_message = error;

        analyses.set(analysis_id, analysis.clone());
        env.storage().persistent().set(&storage_keys::analyses(), &analyses);
        env.storage().persistent().set(&storage_keys::analysis_status(analysis_id), &status);

        // Update metrics
        let mut metrics: AnalysisMetrics =
            env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();

        match new_status {
            AnalysisStatus::Completed => {
                metrics.successful_analyses += 1;
                if analysis.latency_ms > 0 {
                    // Update running average latency
                    metrics.average_latency_ms = (metrics.average_latency_ms
                        * (metrics.successful_analyses - 1)
                        + analysis.latency_ms)
                        / metrics.successful_analyses;
                }
            }
            AnalysisStatus::Failed => metrics.failed_analyses += 1,
            AnalysisStatus::TimedOut => metrics.timed_out_analyses += 1,
            _ => {}
        }

        env.storage().persistent().set(&storage_keys::metrics(), &metrics);

        // If analysis completed successfully, generate recommendation
        if new_status == AnalysisStatus::Completed {
            // Parse raw output into a map (simplified for this framework)
            let mut ai_output: Map<Symbol, u32> = Map::new(&env);
            if analysis.raw_output.len() > 0 {
                // In real implementation, properly deserialize the output
                ai_output.set(symbol_short!("action"), analysis.raw_output.get(0).unwrap_or(0) as u32);
                ai_output.set(symbol_short!("conf"), 85); // Example confidence score
            }

            if let Ok(recommendation) =
                RecommendationEngine::generate_recommendation(&env, &analysis, &ai_output)
            {
                let mut recommendations: Map<u64, Recommendation> = env
                    .storage()
                    .persistent()
                    .get(&storage_keys::recommendations())
                    .unwrap_or_else(|| Map::new(&env));

                let rec_id = recommendation.recommendation_id;
                recommendations.set(rec_id, recommendation);
                env.storage().persistent().set(&storage_keys::recommendations(), &recommendations);

                env.events()
                    .publish((RECOMMEND, analysis.portfolio_id, analysis_id), rec_id);
            }
        }

        Ok(OK)
    }

    /// Handle timeout for an analysis that exceeded its trigger's timeout
    pub fn process_timeout(env: Env, analysis_id: u64) -> Result<Symbol, Error> {
        let mut analyses: Map<u64, AnalysisResult> = env
            .storage()
            .persistent()
            .get(&storage_keys::analyses())
            .ok_or(Error::NotFound)?;

        let mut analysis = analyses.get(analysis_id).ok_or(Error::NotFound)?;

        // Only timeout pending or in-progress analyses
        if analysis.status != AnalysisStatus::Pending as u32
            && analysis.status != AnalysisStatus::InProgress as u32
        {
            return Err(Error::InvalidState);
        }

        analysis.status = AnalysisStatus::TimedOut as u32;
        analysis.error_message = Some(TIMEOUT);

        analyses.set(analysis_id, analysis);
        env.storage().persistent().set(&storage_keys::analyses(), &analyses);
        env.storage()
            .persistent()
            .set(&storage_keys::analysis_status(analysis_id), &(AnalysisStatus::TimedOut as u32));

        // Update metrics
        let mut metrics: AnalysisMetrics =
            env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();
        metrics.timed_out_analyses += 1;
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);

        env.events().publish((TIMEOUT,), analysis_id);

        Ok(OK)
    }

    /// Accept or reject a recommendation
    pub fn process_recommendation_feedback(
        env: Env,
        recommendation_id: u64,
        accepted: bool,
        responder: Address,
    ) -> Result<Symbol, Error> {
        responder.require_auth();

        let mut recommendations: Map<u64, Recommendation> = env
            .storage()
            .persistent()
            .get(&storage_keys::recommendations())
            .ok_or(Error::NotFound)?;

        let mut rec = recommendations.get(recommendation_id).ok_or(Error::NotFound)?;
        rec.accepted = Some(accepted);

        recommendations.set(recommendation_id, rec);
        env.storage().persistent().set(&storage_keys::recommendations(), &recommendations);

        // Update metrics
        let mut metrics: AnalysisMetrics =
            env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default();
        if accepted {
            metrics.recommendations_accepted += 1;
        } else {
            metrics.recommendations_rejected += 1;
        }
        env.storage().persistent().set(&storage_keys::metrics(), &metrics);

        Ok(OK)
    }

    /// Get all analysis results for a portfolio
    pub fn get_portfolio_analyses(env: Env, portfolio_id: Symbol) -> Vec<AnalysisResult> {
        let analyses: Map<u64, AnalysisResult> = env
            .storage()
            .persistent()
            .get(&storage_keys::analyses())
            .unwrap_or_else(|| Map::new(&env));

        let mut results = Vec::new(&env);
        for (_, analysis) in analyses.iter() {
            if analysis.portfolio_id == portfolio_id {
                results.push_back(analysis);
            }
        }
        results
    }

    /// Get all recommendations for a portfolio
    pub fn get_portfolio_recommendations(env: Env, portfolio_id: Symbol) -> Vec<Recommendation> {
        let recommendations: Map<u64, Recommendation> = env
            .storage()
            .persistent()
            .get(&storage_keys::recommendations())
            .unwrap_or_else(|| Map::new(&env));

        let mut results = Vec::new(&env);
        for (_, rec) in recommendations.iter() {
            if rec.portfolio_id == portfolio_id {
                results.push_back(rec);
            }
        }
        results
    }

    /// Get current analysis metrics
    pub fn get_metrics(env: Env) -> AnalysisMetrics {
        env.storage().persistent().get(&storage_keys::metrics()).unwrap_or_default()
    }

    /// Subscribe to portfolio events
    pub fn subscribe(env: Env, portfolio_id: Symbol, subscriber: Address) -> Result<Symbol, Error> {
        subscriber.require_auth();

        let subs_key = storage_keys::subscribers(portfolio_id);
        let mut subscribers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&subs_key)
            .unwrap_or_else(|| Vec::new(&env));

        if !subscribers.contains(&subscriber) {
            subscribers.push_back(subscriber);
            env.storage().persistent().set(&subs_key, &subscribers);
        }

        Ok(OK)
    }

    /// Unsubscribe from portfolio events
    pub fn unsubscribe(env: Env, portfolio_id: Symbol, subscriber: Address) -> Result<Symbol, Error> {
        subscriber.require_auth();

        let subs_key = storage_keys::subscribers(portfolio_id);
        let mut subscribers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&subs_key)
            .ok_or(Error::NotFound)?;

        // Remove subscriber if present
        if let Some(index) = subscribers.first_index_of(&subscriber) {
            subscribers.remove(index);
            env.storage().persistent().set(&subs_key, &subscribers);
        }

        Ok(OK)
    }

    /// Get all registered triggers
    pub fn get_all_triggers(env: Env) -> Vec<AITrigger> {
        let triggers: Map<Symbol, AITrigger> = env
            .storage()
            .persistent()
            .get(&storage_keys::triggers())
            .unwrap_or_else(|| Map::new(&env));

        let mut results = Vec::new(&env);
        for (_, trigger) in triggers.iter() {
            results.push_back(trigger);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{symbol_short, testutils::Address as _, vec, Address, Bytes, Env, U256};

    #[test]
    fn test_initialize() {
        let env = Env::default();
        let result = EventsContract::initialize(env);
        assert_eq!(result, OK);
    }

    #[test]
    fn test_add_and_remove_trigger() {
        let env = Env::default();
        env.mock_all_auths();
        EventsContract::initialize(env.clone());

        let owner = Address::generate(&env);
        let trigger_id = symbol_short!("trig001");
        let event_types = vec![
            &env,
            EventType::TradeExecuted as u32,
            EventType::PortfolioRebalance as u32,
        ];

        let trigger = AITrigger {
            trigger_id,
            name: symbol_short!("testtrig"),
            event_types,
            threshold: Some(U256::from_u32(&env, 1000)),
            operator: Some(ComparisonOperator::GreaterThan as u32),
            ai_service_endpoint: Address::generate(&env),
            timeout: 30000,
            is_active: true,
            owner: owner.clone(),
        };

        // Add the trigger
        let result = EventsContract::add_trigger(env.clone(), trigger);
        assert!(result.is_ok());

        // Verify it was stored
        let triggers = EventsContract::get_all_triggers(env.clone());
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers.get(0).unwrap().trigger_id, trigger_id);

        // Remove the trigger
        let remove_result = EventsContract::remove_trigger(env.clone(), trigger_id, owner);
        assert!(remove_result.is_ok());

        // Verify it was removed
        let triggers_after = EventsContract::get_all_triggers(env.clone());
        assert_eq!(triggers_after.len(), 0);
    }

    #[test]
    fn test_trigger_evaluator() {
        // Test threshold evaluation
        let env = Env::default();
        let owner = Address::generate(&env);
        let trigger = AITrigger {
            trigger_id: symbol_short!("trig001"),
            name: symbol_short!("test"),
            event_types: vec![&env, EventType::PriceThresholdCrossed as u32],
            threshold: Some(U256::from_u32(&env, 100)),
            operator: Some(ComparisonOperator::GreaterThan as u32),
            ai_service_endpoint: Address::generate(&env),
            timeout: 5000,
            is_active: true,
            owner,
        };

        // Should trigger when value exceeds threshold
        let should_fire = TriggerEvaluator::evaluate(
            &trigger,
            EventType::PriceThresholdCrossed,
            Some(U256::from_u32(&env, 150)),
        );
        assert!(should_fire);

        // Should not trigger when value is below threshold
        let should_not_fire = TriggerEvaluator::evaluate(
            &trigger,
            EventType::PriceThresholdCrossed,
            Some(U256::from_u32(&env, 50)),
        );
        assert!(!should_not_fire);
    }

    #[test]
    fn test_process_event() {
        let env = Env::default();
        env.mock_all_auths();
        env.budget().reset_unlimited();
        EventsContract::initialize(env.clone());

        let owner = Address::generate(&env);
        let ai_service = Address::generate(&env);

        // Create and add a trigger
        let trigger = AITrigger {
            trigger_id: symbol_short!("trig001"),
            name: symbol_short!("market"),
            event_types: vec![&env, EventType::VolatilitySpike as u32],
            threshold: None,
            operator: None,
            ai_service_endpoint: ai_service,
            timeout: 10000,
            is_active: true,
            owner,
        };

        EventsContract::add_trigger(env.clone(), trigger).unwrap();

        // Process an event that should trigger the analysis
        let portfolio_id = symbol_short!("port001");
        let event_data = Bytes::from_array(&env, &[0x01, 0x02, 0x03]);
        let analyses = EventsContract::process_event(
            env.clone(),
            portfolio_id,
            EventType::VolatilitySpike as u32,
            event_data,
            None,
        )
        .unwrap();

        // Should have triggered one analysis
        assert_eq!(analyses.len(), 1);

        // Verify the analysis was stored
        let portfolio_analyses = EventsContract::get_portfolio_analyses(env.clone(), portfolio_id);
        assert_eq!(portfolio_analyses.len(), 1);
        assert_eq!(portfolio_analyses.get(0).unwrap().status, AnalysisStatus::Pending as u32);
    }

    #[test]
    fn test_update_analysis_status_and_metrics() {
        let env = Env::default();
        env.mock_all_auths();
        env.budget().reset_unlimited();
        EventsContract::initialize(env.clone());

        let owner = Address::generate(&env);
        let ai_service = Address::generate(&env);

        // Add trigger
        let trigger = AITrigger {
            trigger_id: symbol_short!("trig001"),
            name: symbol_short!("test"),
            event_types: vec![&env, EventType::TradeExecuted as u32],
            threshold: None,
            operator: None,
            ai_service_endpoint: ai_service,
            timeout: 5000,
            is_active: true,
            owner,
        };
        EventsContract::add_trigger(env.clone(), trigger).unwrap();

        // Process event to create analysis
        let portfolio_id = symbol_short!("port001");
        let analyses = EventsContract::process_event(
            env.clone(),
            portfolio_id,
            EventType::TradeExecuted as u32,
            Bytes::from_array(&env, &[0x01]),
            None,
        )
        .unwrap();

        let analysis_id = analyses.get(0).unwrap();

        // Update status to completed
        let update_result = EventsContract::update_analysis_status(
            env.clone(),
            analysis_id,
            AnalysisStatus::Completed as u32,
            Some(1500u64),
            Some(Bytes::from_array(&env, &[0x01, 0x02])),
            None,
        );
        assert!(update_result.is_ok());

        // Check metrics were updated
        let metrics = EventsContract::get_metrics(env.clone());
        assert_eq!(metrics.total_analyses, 1);
        assert_eq!(metrics.successful_analyses, 1);
        assert_eq!(metrics.average_latency_ms, 1500);
    }

    #[test]
    fn test_timeout_handling() {
        let env = Env::default();
        env.mock_all_auths();
        env.budget().reset_unlimited();
        EventsContract::initialize(env.clone());

        let owner = Address::generate(&env);
        let ai_service = Address::generate(&env);

        let trigger = AITrigger {
            trigger_id: symbol_short!("trig001"),
            name: symbol_short!("timeout"),
            event_types: vec![&env, EventType::LiquidityChange as u32],
            threshold: None,
            operator: None,
            ai_service_endpoint: ai_service,
            timeout: 1000,
            is_active: true,
            owner,
        };
        EventsContract::add_trigger(env.clone(), trigger).unwrap();

        // Create an analysis
        let portfolio_id = symbol_short!("port001");
        let analyses = EventsContract::process_event(
            env.clone(),
            portfolio_id,
            EventType::LiquidityChange as u32,
            Bytes::from_array(&env, &[0x01]),
            None,
        )
        .unwrap();

        let analysis_id = analyses.get(0).unwrap();

        // Process timeout
        let timeout_result = EventsContract::process_timeout(env.clone(), analysis_id);
        assert!(timeout_result.is_ok());

        // Verify analysis is marked as timed out
        let portfolio_analyses = EventsContract::get_portfolio_analyses(env.clone(), portfolio_id);
        assert_eq!(portfolio_analyses.get(0).unwrap().status, AnalysisStatus::TimedOut as u32);
        assert_eq!(portfolio_analyses.get(0).unwrap().error_message, Some(TIMEOUT));

        // Check metrics
        let metrics = EventsContract::get_metrics(env.clone());
        assert_eq!(metrics.timed_out_analyses, 1);
    }

    #[test]
    fn test_subscribe_unsubscribe() {
        let env = Env::default();
        env.mock_all_auths();
        let portfolio_id = symbol_short!("port001");
        let subscriber = Address::generate(&env);

        // Subscribe
        let sub_result = EventsContract::subscribe(env.clone(), portfolio_id, subscriber.clone());
        assert!(sub_result.is_ok());

        // Unsubscribe
        let unsub_result = EventsContract::unsubscribe(env.clone(), portfolio_id, subscriber);
        assert!(unsub_result.is_ok());
    }
}
