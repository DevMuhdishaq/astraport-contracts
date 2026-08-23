//! Unit tests for the advanced subscription system (issue #7):
//! filtering, retry backoff, batch delivery, acknowledgment and metrics.

use crate::{
    subscriptions::{
        numeric_detail, DeliveryRecord, DeliveryStatus, EventFilter, FilterCondition,
        SubscriptionPreferences, SubscriptionStatus, RETRY_INITIAL_SECS, RETRY_MAX_SECS,
    },
    ComparisonOperator, EventsContract, EventsContractClient, PortfolioEventType,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Map, Symbol, Vec,
};

macro_rules! setup {
    () => {{
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, EventsContract);
        let client = EventsContractClient::new(&env, &contract_id);
        (env, client)
    }};
}

fn filter_all(env: &Env) -> EventFilter {
    EventFilter {
        event_types: Vec::new(env),
        min_severity: 0,
        conditions: Vec::new(env),
    }
}

fn details_value(env: &Env, key: &str, value: i128) -> Map<Symbol, Bytes> {
    let mut m = Map::new(env);
    m.set(Symbol::new(env, key), numeric_detail(env, value));
    m
}

#[test]
fn immediate_delivery_end_to_end() {
    let (env, client) = setup!();
    let sub = Address::generate(&env);
    let portfolio = Symbol::new(&env, "pf1");

    let id = client
        .try_create_subscription(
            &portfolio,
            &sub,
            &filter_all(&env),
            &SubscriptionPreferences::immediate(),
        )
        .unwrap()
        .unwrap();

    let event_id = client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::Rebalanced,
            &1u32,
            &Map::new(&env),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();

    // Backoff schedules the first attempt RETRY_INITIAL_SECS out.
    let records = client.get_delivery_records(&sub);
    assert_eq!(records.len(), 1);
    assert_eq!(records.get(0).unwrap().status, DeliveryStatus::Pending);

    env.ledger().with_mut(|l| l.timestamp += RETRY_INITIAL_SECS);
    let delivered = client.try_deliver_pending(&sub).unwrap().unwrap();
    assert_eq!(delivered.len(), 1);

    client.try_acknowledge(&sub, &event_id).unwrap().unwrap();

    let rec: DeliveryRecord = client.get_delivery_records(&sub).get(0).unwrap();
    assert_eq!(rec.status, DeliveryStatus::Acknowledged);
    assert_eq!(rec.attempts, 1);

    let managed = client.get_managed_subscription(&id).unwrap();
    assert_eq!(managed.total_delivered, 1);
    assert!(managed.last_event_received_at.is_some());

    let metrics = client.get_delivery_metrics();
    assert_eq!(metrics.total_dispatched, 1);
    assert_eq!(metrics.total_delivered, 1);
    assert_eq!(metrics.total_acknowledged, 1);
}

#[test]
fn filter_by_type_and_severity_and_predicate() {
    let (env, client) = setup!();
    let sub = Address::generate(&env);
    let portfolio = Symbol::new(&env, "pf2");

    let mut types = Vec::new(&env);
    types.push_back(PortfolioEventType::TradeExecuted);
    let mut conds = Vec::new(&env);
    conds.push_back(FilterCondition {
        key: Symbol::new(&env, "amount"),
        op: ComparisonOperator::GreaterThan,
        value: 900,
    });
    let filter = EventFilter {
        event_types: types,
        min_severity: 2,
        conditions: conds,
    };

    client
        .try_create_subscription(
            &portfolio,
            &sub,
            &filter,
            &SubscriptionPreferences::immediate(),
        )
        .unwrap()
        .unwrap();

    // Wrong type -> no delivery record.
    client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::Rebalanced,
            &5u32,
            &details_value(&env, "amount", 5000),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();
    assert_eq!(client.get_delivery_records(&sub).len(), 0);

    // Right type but severity too low.
    client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::TradeExecuted,
            &1u32,
            &details_value(&env, "amount", 5000),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();
    assert_eq!(client.get_delivery_records(&sub).len(), 0);

    // Severity ok but predicate fails.
    client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::TradeExecuted,
            &3u32,
            &details_value(&env, "amount", 100),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();
    assert_eq!(client.get_delivery_records(&sub).len(), 0);

    // Everything matches -> queued.
    let event_id = client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::TradeExecuted,
            &3u32,
            &details_value(&env, "amount", 1500),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();
    assert_eq!(client.get_delivery_records(&sub).len(), 1);
    assert_eq!(client.get_event_severity(&event_id), 3);
}

#[test]
fn retry_backoff_doubles_then_delivers() {
    let (env, client) = setup!();
    let sub = Address::generate(&env);
    let portfolio = Symbol::new(&env, "pf3");

    client
        .try_create_subscription(
            &portfolio,
            &sub,
            &filter_all(&env),
            &SubscriptionPreferences::immediate(),
        )
        .unwrap()
        .unwrap();
    client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::Rebalanced,
            &1u32,
            &Map::new(&env),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();

    // Failures schedule exponentially backing-off retries: 1s, 2s, 4s.
    client
        .try_mark_delivery_failed(&sub, &1u64)
        .unwrap()
        .unwrap();
    let rec = client.get_delivery_records(&sub).get(0).unwrap();
    assert_eq!(rec.attempts, 1);
    assert_eq!(rec.next_retry_at - env.ledger().timestamp(), 1);

    client
        .try_mark_delivery_failed(&sub, &1u64)
        .unwrap()
        .unwrap();
    let rec = client.get_delivery_records(&sub).get(0).unwrap();
    assert_eq!(rec.next_retry_at - env.ledger().timestamp(), 2);

    client
        .try_mark_delivery_failed(&sub, &1u64)
        .unwrap()
        .unwrap();
    let rec = client.get_delivery_records(&sub).get(0).unwrap();
    assert_eq!(rec.next_retry_at - env.ledger().timestamp(), 4);

    // Deliver after waiting out the backoff.
    env.ledger().with_mut(|l| l.timestamp += 4);
    let delivered = client.try_deliver_pending(&sub).unwrap().unwrap();
    assert_eq!(delivered.len(), 1);
    let rec = client.get_delivery_records(&sub).get(0).unwrap();
    assert_eq!(rec.status, DeliveryStatus::Delivered);
    assert_eq!(rec.attempts, 4);

    let metrics = client.get_delivery_metrics();
    assert_eq!(metrics.total_failed, 3);
    assert_eq!(metrics.total_retry_attempts, 3);
}

#[test]
fn backoff_delay_saturates_at_max() {
    assert_eq!(crate::subscriptions::backoff_delay(0), RETRY_INITIAL_SECS);
    assert_eq!(crate::subscriptions::backoff_delay(1), 2);
    assert_eq!(crate::subscriptions::backoff_delay(2), 4);
    assert_eq!(crate::subscriptions::backoff_delay(12), RETRY_MAX_SECS);
    assert_eq!(
        crate::subscriptions::backoff_delay(u32::MAX),
        RETRY_MAX_SECS
    );
}

#[test]
fn batch_mode_accumulates_and_flushes_on_size() {
    let (env, client) = setup!();
    let sub = Address::generate(&env);
    let portfolio = Symbol::new(&env, "pf4");

    client
        .try_create_subscription(
            &portfolio,
            &sub,
            &filter_all(&env),
            &SubscriptionPreferences::batch(3, 600),
        )
        .unwrap()
        .unwrap();

    for _ in 0..3 {
        client
            .try_dispatch_event(
                &portfolio,
                &PortfolioEventType::Rebalanced,
                &1u32,
                &Map::new(&env),
                &Bytes::new(&env),
            )
            .unwrap()
            .unwrap();
    }

    // Batch size reached on the third dispatch -> auto-flush delivered all.
    let records = client.get_delivery_records(&sub);
    assert_eq!(records.len(), 3);
    for i in 0..records.len() {
        assert_eq!(records.get(i).unwrap().status, DeliveryStatus::Delivered);
    }

    // Nothing left pending afterwards.
    env.ledger().with_mut(|l| l.timestamp += 10);
    assert_eq!(client.try_deliver_pending(&sub).unwrap().unwrap().len(), 0);
}

#[test]
fn pause_stops_matching_until_resumed() {
    let (env, client) = setup!();
    let sub = Address::generate(&env);
    let portfolio = Symbol::new(&env, "pf5");

    let id = client
        .try_create_subscription(
            &portfolio,
            &sub,
            &filter_all(&env),
            &SubscriptionPreferences::immediate(),
        )
        .unwrap()
        .unwrap();

    client.try_pause_subscription(&id).unwrap().unwrap();
    client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::Rebalanced,
            &1u32,
            &Map::new(&env),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();
    assert_eq!(client.get_delivery_records(&sub).len(), 0);

    client.try_resume_subscription(&id).unwrap().unwrap();
    client
        .try_dispatch_event(
            &portfolio,
            &PortfolioEventType::Rebalanced,
            &1u32,
            &Map::new(&env),
            &Bytes::new(&env),
        )
        .unwrap()
        .unwrap();
    assert_eq!(client.get_delivery_records(&sub).len(), 1);

    let managed = client.get_managed_subscription(&id).unwrap();
    assert_eq!(managed.status, SubscriptionStatus::Active);
}
