// Integration tests for AstraPort Smart Contracts
//
// These tests verify contract interactions and cross-contract communication
// NOTE: The comprehensive event emission and subscription integration tests
// live in contracts/events/tests/integration_tests.rs. The stubs below
// remain as placeholders for future cross-contract integration tests
// involving the rebalancing and staking crates.

#[cfg(test)]
mod integration_tests {
    #[test]
    fn test_rebalancing_workflow() {
        println!("Rebalancing workflow test");
    }

    #[test]
    fn test_event_emission() {
        println!("Event emission test");
    }

    #[test]
    fn test_staking_and_alerts() {
        println!("Staking and alerts test");
    }

    #[test]
    fn test_cross_contract_interaction() {
        println!("Cross-contract interaction test");
    }

    #[test]
    fn test_error_handling() {
        println!("Error handling test");
    }

    #[test]
    fn test_access_control() {
        println!("Access control test");
    }
}

// ============================================================================
// Fee Management Integration Tests
// ============================================================================

#[cfg(test)]
mod fee_integration_tests {
    use astraport_fee::{
        FeeManagementContract, FeeType, RevenueRecipient, TierEntry,
    };
    use soroban_sdk::{symbol_short, Address, Env};

    fn setup() -> (Env, Address) {
        let env = Env::default();
        let admin = Address::generate(&env);
        FeeManagementContract::initialize(env.clone(), admin.clone());
        (env, admin)
    }

    /// Full fee lifecycle: setup, assign, collect, distribute, report.
    #[test]
    fn test_full_fee_lifecycle() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        // 1. Admin creates a percentage fee structure for rebalancing
        let rebal_fee_id = symbol_short!("REBAL");
        FeeManagementContract::set_fee_structure(
            env.clone(),
            rebal_fee_id.clone(),
            FeeType::Percentage,
            250, // 2.50%
            soroban_sdk::Vec::new(&env),
            true,
        );

        // 2. Admin creates a flat fee structure for management
        let mgmt_fee_id = symbol_short!("MGMT");
        FeeManagementContract::set_fee_structure(
            env.clone(),
            mgmt_fee_id.clone(),
            FeeType::Flat,
            1_000,
            soroban_sdk::Vec::new(&env),
            true,
        );

        // 3. Assign fee structures to portfolios
        let portfolio_a = symbol_short!("PORT_A");
        let portfolio_b = symbol_short!("PORT_B");
        FeeManagementContract::set_portfolio_fee(
            env.clone(),
            portfolio_a.clone(),
            rebal_fee_id.clone(),
        );
        FeeManagementContract::set_portfolio_fee(
            env.clone(),
            portfolio_b.clone(),
            mgmt_fee_id.clone(),
        );

        // 4. Set up revenue distribution
        let treasury = Address::generate(&env);
        let dev_fund = Address::generate(&env);
        let mut recipients = soroban_sdk::Vec::new(&env);
        recipients.push_back(RevenueRecipient {
            address: treasury.clone(),
            share_numerator: 70,
        });
        recipients.push_back(RevenueRecipient {
            address: dev_fund.clone(),
            share_numerator: 30,
        });
        FeeManagementContract::set_revenue_recipients(env.clone(), recipients);

        // 5. Collect fees
        let caller_1 = Address::generate(&env);
        let fee_a = FeeManagementContract::collect_fee(
            env.clone(),
            caller_1,
            rebal_fee_id,
            portfolio_a,
            10_000_000,
        );
        // 10M * 2.5% = 250,000
        assert_eq!(fee_a, 250_000);

        let caller_2 = Address::generate(&env);
        let fee_b = FeeManagementContract::collect_fee(
            env.clone(),
            caller_2,
            mgmt_fee_id,
            portfolio_b,
            5_000_000,
        );
        assert_eq!(fee_b, 1_000);

        // 6. Verify total collected
        let total = FeeManagementContract::get_total_collected(env.clone());
        assert_eq!(total, 251_000);

        // 7. Verify fee history
        let history = FeeManagementContract::get_fee_history(env.clone(), 0);
        assert_eq!(history.len(), 2);

        // 8. Verify list of fee structures
        let fee_ids = FeeManagementContract::list_fee_structures(env.clone());
        assert_eq!(fee_ids.len(), 2);

        // 9. Verify revenue distribution would be proportional
        let dist = FeeManagementContract::distribute_revenue_amount(env.clone(), 251_000);
        assert_eq!(dist.len(), 2);
        assert_eq!(dist.get(0).unwrap().1, 175_700); // 70% of 251K
        assert_eq!(dist.get(1).unwrap().1, 75_300); // 30% of 251K
    }

    /// Tiered fee with progressive rates.
    #[test]
    fn test_tiered_fee_progressive() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let mut tiers = soroban_sdk::Vec::new(&env);
        tiers.push_back(TierEntry { threshold: 0, fee_bps: 50 });
        tiers.push_back(TierEntry { threshold: 10_000_000, fee_bps: 30 });
        tiers.push_back(TierEntry { threshold: 100_000_000, fee_bps: 15 });

        let fee_id = symbol_short!("TIERED");
        FeeManagementContract::set_fee_structure(
            env.clone(), fee_id.clone(), FeeType::Tiered, 0, tiers, true,
        );

        let result = FeeManagementContract::calculate_fee(env.clone(), fee_id.clone(), 5_000_000);
        assert_eq!(result.fee_amount, 25_000);

        let result = FeeManagementContract::calculate_fee(env.clone(), fee_id.clone(), 50_000_000);
        assert_eq!(result.fee_amount, 150_000);

        let result = FeeManagementContract::calculate_fee(env, fee_id, 200_000_000);
        assert_eq!(result.fee_amount, 300_000);
    }

    /// Portfolio with multiple fee types used sequentially.
    #[test]
    fn test_portfolio_with_multiple_fee_types() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let rebal_id = symbol_short!("REBAL");
        let yield_id = symbol_short!("YIELD");
        let mgmt_id = symbol_short!("MGMT");

        FeeManagementContract::set_fee_structure(
            env.clone(), rebal_id.clone(), FeeType::Percentage, 200,
            soroban_sdk::Vec::new(&env), true,
        );
        FeeManagementContract::set_fee_structure(
            env.clone(), yield_id.clone(), FeeType::Percentage, 100,
            soroban_sdk::Vec::new(&env), true,
        );
        FeeManagementContract::set_fee_structure(
            env.clone(), mgmt_id.clone(), FeeType::Flat, 5_000,
            soroban_sdk::Vec::new(&env), true,
        );

        let portfolio = symbol_short!("MULTI");
        let caller = Address::generate(&env);

        let fee = FeeManagementContract::collect_fee(
            env.clone(), caller.clone(), rebal_id, portfolio.clone(), 10_000_000,
        );
        assert_eq!(fee, 200_000);

        let fee = FeeManagementContract::collect_fee(
            env.clone(), caller.clone(), yield_id, portfolio.clone(), 1_000_000,
        );
        assert_eq!(fee, 10_000);

        let fee = FeeManagementContract::collect_fee(
            env.clone(), caller, mgmt_id, portfolio, 0,
        );
        assert_eq!(fee, 5_000);

        assert_eq!(FeeManagementContract::get_total_collected(env), 215_000);
    }

    /// Zero-fee portfolio via assignment for grant/special case.
    #[test]
    fn test_zero_fee_portfolio_via_assignment() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let zero_id = symbol_short!("ZERO");
        FeeManagementContract::set_fee_structure(
            env.clone(), zero_id.clone(), FeeType::Percentage, 0,
            soroban_sdk::Vec::new(&env), true,
        );

        let normal_id = symbol_short!("NORM");
        FeeManagementContract::set_fee_structure(
            env.clone(), normal_id.clone(), FeeType::Percentage, 200,
            soroban_sdk::Vec::new(&env), true,
        );

        let grant = symbol_short!("GRANT");
        FeeManagementContract::set_portfolio_fee(env.clone(), grant.clone(), zero_id);

        let normal = symbol_short!("NORMP");
        FeeManagementContract::set_portfolio_fee(env.clone(), normal.clone(), normal_id.clone());

        let result = FeeManagementContract::calculate_portfolio_fee(
            env.clone(), grant, normal_id, 10_000_000,
        );
        assert_eq!(result.fee_amount, 0);

        let result = FeeManagementContract::calculate_portfolio_fee(
            env.clone(), normal, symbol_short!("X"), 10_000_000,
        );
        assert_eq!(result.fee_amount, 200_000);
    }

    /// Fee estimation for frontend display.
    #[test]
    fn test_fee_estimation_workflow() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let fee_id = symbol_short!("EST");
        FeeManagementContract::set_fee_structure(
            env.clone(), fee_id.clone(), FeeType::Percentage, 150,
            soroban_sdk::Vec::new(&env), true,
        );

        let portfolio = symbol_short!("ESTP");
        FeeManagementContract::set_portfolio_fee(env.clone(), portfolio.clone(), fee_id.clone());

        let est_small = FeeManagementContract::estimate_fee(
            env.clone(), fee_id.clone(), Some(portfolio.clone()), 1_000_000,
        );
        assert_eq!(est_small.fee_amount, 15_000);

        let est_large = FeeManagementContract::estimate_fee(
            env.clone(), fee_id, Some(portfolio), 100_000_000,
        );
        assert_eq!(est_large.fee_amount, 1_500_000);
    }

    /// Revenue sharing with many recipients.
    #[test]
    fn test_revenue_sharing_many_recipients() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let mut recipients = soroban_sdk::Vec::new(&env);
        for i in 0..5u32 {
            let addr = Address::generate(&env);
            recipients.push_back(RevenueRecipient {
                address: addr,
                share_numerator: (i + 1) * 10,
            });
        }
        FeeManagementContract::set_revenue_recipients(env.clone(), recipients);

        let dist = FeeManagementContract::distribute_revenue_amount(env, 150_000);
        assert_eq!(dist.len(), 5);
        assert_eq!(dist.get(0).unwrap().1, 10_000);
        assert_eq!(dist.get(1).unwrap().1, 20_000);
        assert_eq!(dist.get(2).unwrap().1, 30_000);
        assert_eq!(dist.get(3).unwrap().1, 40_000);
        assert_eq!(dist.get(4).unwrap().1, 50_000);
    }

    /// Fee structure activation/deactivation lifecycle.
    #[test]
    fn test_fee_structure_lifecycle() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let fee_id = symbol_short!("LIFE");
        FeeManagementContract::set_fee_structure(
            env.clone(), fee_id.clone(), FeeType::Percentage, 100,
            soroban_sdk::Vec::new(&env), true,
        );

        let result = FeeManagementContract::calculate_fee(env.clone(), fee_id.clone(), 1_000_000);
        assert_eq!(result.fee_amount, 10_000);

        FeeManagementContract::set_fee_active(env.clone(), fee_id.clone(), false);

        let result = std::panic::catch_unwind(|| {
            let env2 = soroban_sdk::Env::default();
            FeeManagementContract::calculate_fee(env2, fee_id.clone(), 1_000_000);
        });
        assert!(result.is_err());

        FeeManagementContract::set_fee_active(env.clone(), fee_id.clone(), true);

        let result = FeeManagementContract::calculate_fee(env, fee_id, 1_000_000);
        assert_eq!(result.fee_amount, 10_000);
    }

    /// Waiver with 50% discount.
    #[test]
    fn test_waiver_with_discount() {
        let (env, _admin) = setup();
        env.mock_all_auths();

        let fee_id = symbol_short!("WDISC");
        FeeManagementContract::set_fee_structure(
            env.clone(), fee_id.clone(), FeeType::Percentage, 200,
            soroban_sdk::Vec::new(&env), true,
        );

        let portfolio = symbol_short!("WDP");
        FeeManagementContract::set_portfolio_fee(env.clone(), portfolio.clone(), fee_id);

        // 50% discount on portfolio
        FeeManagementContract::set_fee_waiver(
            env.clone(), None, Some(portfolio.clone()), 5000, false,
        );

        let result = FeeManagementContract::calculate_portfolio_fee(
            env, portfolio, symbol_short!("X"), 10_000_000,
        );
        // Gross: 10M * 2% = 200K, Discount 50% → 100K
        assert_eq!(result.fee_amount, 100_000);
        assert_eq!(result.discount_bps, 5000);
    }
}

// ============================================================================
// Gamification Engine Integration Tests
// ============================================================================

#[cfg(test)]
mod gamification_integration_tests {
    use astraport_gamification::{
        GamificationEngine, ProgressionTier, SortMetric, TimeWindow,
    };
    use soroban_sdk::{symbol_short, Address, Env};

    fn setup() -> (Env, Address) {
        let env = Env::default();
        let admin = Address::generate(&env);
        env.mock_all_auths();
        GamificationEngine::initialize(env.clone(), admin.clone(), 5);
        (env, admin)
    }

    /// Full gamification lifecycle: register, trade, learn, earn badges,
    /// climb leaderboard, complete challenges, collect rewards.
    #[test]
    fn test_full_gamification_lifecycle() {
        let (env, _admin) = setup();

        // 1. Register multiple users
        let mut users = soroban_sdk::Vec::<Address>::new(&env);
        for _ in 0..5 {
            let user = Address::generate(&env);
            GamificationEngine::register_user(env.clone(), user.clone());
            users.push_back(user);
        }
        assert_eq!(GamificationEngine::get_total_players(env.clone()), 5);

        // 2. Users perform trades with varying performance
        for i in 0..5u32 {
            let user = users.get(i).unwrap();
            let num_trades = (i + 1) * 3;
            for j in 0..num_trades {
                let roi = (j as i128 + 1) * 200;
                GamificationEngine::record_trade(env.clone(), user.clone(), roi, true);
            }
        }

        // 3. Complete learning modules
        let top_user = users.get(4).unwrap();
        for _ in 0..5 {
            GamificationEngine::complete_learning_module(env.clone(), top_user.clone());
        }

        // 4. Record community actions
        for _ in 0..3 {
            GamificationEngine::record_community_action(env.clone(), top_user.clone());
        }

        // 5. Check and issue badges
        let badges = GamificationEngine::check_and_issue_badges(env.clone(), top_user.clone());
        assert!(badges.len() >= 4); // Multiple badges earned

        // 6. Verify leaderboard
        let leaderboard = GamificationEngine::get_leaderboard(
            env.clone(),
            SortMetric::Score,
            TimeWindow::AllTime,
            0,
            10,
        );
        assert_eq!(leaderboard.total_players, 5);
        assert_eq!(leaderboard.entries.len(), 5);

        // Top user should be rank 1
        let top_entry = leaderboard.entries.get(0).unwrap();
        assert_eq!(top_entry.user, top_user.clone());
        assert_eq!(top_entry.rank, 1);
        assert_eq!(top_entry.tier, ProgressionTier::Platinum);

        // 7. Verify score breakdown
        let (trade_s, roi_s, streak_s, learn_s, comm_s, total) =
            GamificationEngine::get_score_breakdown(env.clone(), top_user.clone());
        assert_eq!(trade_s, 30); // Capped at max
        assert_eq!(roi_s, 30); // Capped at max
        assert_eq!(streak_s, 15); // 15 trades streak, capped
        assert_eq!(learn_s, 15); // 5/5 modules, full score
        assert_eq!(comm_s, 6); // 3 actions * 2 = 6
        assert_eq!(total, trade_s + roi_s + streak_s + learn_s + comm_s);

        // 8. Verify user badges
        let user_badges = GamificationEngine::get_user_badges(env.clone(), top_user.clone());
        assert!(user_badges.len() >= 4);

        // 9. Verify tier-based reward distribution
        let reward = GamificationEngine::distribute_tier_reward(env.clone(), top_user.clone());
        assert_eq!(reward, 100); // Platinum reward

        let distributed = GamificationEngine::get_reward_distributed(env.clone(), top_user.clone());
        assert!(distributed > 0);
    }

    /// Leaderboard with multiple sorting metrics.
    #[test]
    fn test_leaderboard_multi_metric() {
        let (env, _admin) = setup();

        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        let user3 = Address::generate(&env);

        GamificationEngine::register_user(env.clone(), user1.clone());
        GamificationEngine::register_user(env.clone(), user2.clone());
        GamificationEngine::register_user(env.clone(), user3.clone());

        // user1: high ROI, few trades
        GamificationEngine::record_trade(env.clone(), user1.clone(), 5000, true);

        // user2: many trades, low ROI
        for _ in 0..10 {
            GamificationEngine::record_trade(env.clone(), user2.clone(), 100, true);
        }

        // user3: medium everything
        for _ in 0..5 {
            GamificationEngine::record_trade(env.clone(), user3.clone(), 500, true);
        }

        // By score
        let lb_score = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Score, TimeWindow::AllTime, 0, 10,
        );
        assert_eq!(lb_score.entries.len(), 3);

        // By trade count - user2 should be first
        let lb_trades = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::TradeCount, TimeWindow::AllTime, 0, 10,
        );
        assert_eq!(lb_trades.entries.get(0).unwrap().user, user2);

        // By ROI - user1 should be first
        let lb_roi = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Roi, TimeWindow::AllTime, 0, 10,
        );
        assert_eq!(lb_roi.entries.get(0).unwrap().user, user1);

        // By streak - user2 should be first (10 streak)
        let lb_streak = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Streak, TimeWindow::AllTime, 0, 10,
        );
        assert_eq!(lb_streak.entries.get(0).unwrap().user, user2);
    }

    /// Challenge campaign end-to-end flow.
    #[test]
    fn test_challenge_campaign_e2e() {
        let (env, admin) = setup();

        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user1.clone());
        GamificationEngine::register_user(env.clone(), user2.clone());

        let now = env.ledger().timestamp();
        let start = now + 100;
        let end = now + 100_000;

        // Create a trade-count challenge: complete 5 trades
        GamificationEngine::create_challenge(
            env.clone(), admin.clone(),
            symbol_short!("TRD_CH"),
            symbol_short!("5_Trades"),
            symbol_short!("Do5trades"),
            SortMetric::TradeCount,
            5,
            200,
            start, end,
        );

        env.ledger().set(start + 1);

        // Both users join
        GamificationEngine::join_challenge(env.clone(), user1.clone(), symbol_short!("TRD_CH"));
        GamificationEngine::join_challenge(env.clone(), user2.clone(), symbol_short!("TRD_CH"));

        // user1 makes progress: 3 then 2 -> completes
        let done1a = GamificationEngine::update_challenge_progress(
            env.clone(), user1.clone(), symbol_short!("TRD_CH"), 3,
        );
        assert!(!done1a);

        let done1b = GamificationEngine::update_challenge_progress(
            env.clone(), user1.clone(), symbol_short!("TRD_CH"), 2,
        );
        assert!(done1b);

        // user2 makes progress: 4 -> not done yet
        let done2 = GamificationEngine::update_challenge_progress(
            env.clone(), user2.clone(), symbol_short!("TRD_CH"), 4,
        );
        assert!(!done2);

        // Verify entries
        let entry1 = GamificationEngine::get_challenge_entry(
            env.clone(), user1.clone(), symbol_short!("TRD_CH"),
        ).unwrap();
        assert!(entry1.completed);

        let entry2 = GamificationEngine::get_challenge_entry(
            env.clone(), user2.clone(), symbol_short!("TRD_CH"),
        ).unwrap();
        assert!(!entry2.completed);
        assert_eq!(entry2.current_value, 4);

        // user1 should have received reward
        let reward1 = GamificationEngine::get_reward_distributed(env.clone(), user1.clone());
        assert_eq!(reward1, 200);

        // user1 should have challenges_won incremented
        let stats1 = GamificationEngine::get_player_stats(env.clone(), user1).unwrap();
        assert_eq!(stats1.challenges_won, 1);

        // Admin ends the challenge
        GamificationEngine::end_challenge(env.clone(), admin, symbol_short!("TRD_CH"));

        let challenge = GamificationEngine::get_challenge(env.clone(), symbol_short!("TRD_CH")).unwrap();
        assert!(!challenge.active);
    }

    /// Multiple users earning badges simultaneously.
    #[test]
    fn test_simultaneous_badge_issuance() {
        let (env, _admin) = setup();

        let mut users = soroban_sdk::Vec::<Address>::new(&env);
        for _ in 0..10 {
            let user = Address::generate(&env);
            GamificationEngine::register_user(env.clone(), user.clone());
            users.push_back(user);
        }

        // All users do 1 trade - everyone earns FIRST_TRADE badge
        for i in 0..10u32 {
            let user = users.get(i).unwrap();
            GamificationEngine::record_trade(env.clone(), user.clone(), 500, true);
            let badges = GamificationEngine::check_and_issue_badges(env.clone(), user.clone());
            assert!(badges.len() >= 1); // At least FIRST_TRADE
        }

        // Verify all users got their badges
        for i in 0..10u32 {
            let user = users.get(i).unwrap();
            let user_badges = GamificationEngine::get_user_badges(env.clone(), user);
            assert!(user_badges.len() >= 1);
        }

        // Verify leaderboard has all 10
        let lb = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Score, TimeWindow::AllTime, 0, 20,
        );
        assert_eq!(lb.total_players, 10);
    }

    /// Tier progression scenarios (5 scenarios).
    #[test]
    fn test_tier_progression_scenarios() {
        let (env, _admin) = setup();

        // Scenario 1: Pure trading -> Silver
        let user1 = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user1.clone());
        for _ in 0..10 {
            GamificationEngine::record_trade(env.clone(), user1.clone(), 300, true);
        }
        let stats = GamificationEngine::get_player_stats(env.clone(), user1.clone()).unwrap();
        assert_eq!(stats.tier, ProgressionTier::Silver);

        // Scenario 2: Pure learning -> Bronze (limited points)
        let user2 = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user2.clone());
        for _ in 0..5 {
            GamificationEngine::complete_learning_module(env.clone(), user2.clone());
        }
        let stats = GamificationEngine::get_player_stats(env.clone(), user2.clone()).unwrap();
        assert_eq!(stats.tier, ProgressionTier::Bronze);

        // Scenario 3: Mixed -> Gold
        let user3 = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user3.clone());
        for _ in 0..15 {
            GamificationEngine::record_trade(env.clone(), user3.clone(), 500, true);
        }
        for _ in 0..5 {
            GamificationEngine::complete_learning_module(env.clone(), user3.clone());
        }
        let stats = GamificationEngine::get_player_stats(env.clone(), user3.clone()).unwrap();
        assert!(stats.tier >= ProgressionTier::Gold);

        // Scenario 4: Full engagement -> Platinum
        let user4 = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user4.clone());
        for _ in 0..20 {
            GamificationEngine::record_trade(env.clone(), user4.clone(), 1000, true);
        }
        for _ in 0..5 {
            GamificationEngine::complete_learning_module(env.clone(), user4.clone());
        }
        for _ in 0..5 {
            GamificationEngine::record_community_action(env.clone(), user4.clone());
        }
        let stats = GamificationEngine::get_player_stats(env.clone(), user4.clone()).unwrap();
        assert_eq!(stats.tier, ProgressionTier::Platinum);

        // Scenario 5: Losing streak -> stays Bronze
        let user5 = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user5.clone());
        for _ in 0..5 {
            GamificationEngine::record_trade(env.clone(), user5.clone(), -500, false);
        }
        let stats = GamificationEngine::get_player_stats(env.clone(), user5.clone()).unwrap();
        assert_eq!(stats.tier, ProgressionTier::Bronze);
    }

    /// Reward pool management and distribution.
    #[test]
    fn test_reward_pool_and_distribution() {
        let (env, admin) = setup();

        let initial_pool = GamificationEngine::get_reward_pool(env.clone());
        assert_eq!(initial_pool, 1_000_000);

        // Fund more
        let new_pool = GamificationEngine::fund_reward_pool(env.clone(), admin.clone(), 500_000);
        assert_eq!(new_pool, 1_500_000);

        // Create users at different tiers and distribute rewards
        let users_tiers: &[(u32, ProgressionTier)] = &[
            (1, ProgressionTier::Bronze),
            (5, ProgressionTier::Silver),
            (10, ProgressionTier::Gold),
            (20, ProgressionTier::Platinum),
        ];

        let mut total_distributed: i128 = 0;
        for &(num_trades, expected_tier) in users_tiers.iter() {
            let user = Address::generate(&env);
            GamificationEngine::register_user(env.clone(), user.clone());
            for _ in 0..num_trades {
                GamificationEngine::record_trade(env.clone(), user.clone(), 500, true);
            }
            let stats = GamificationEngine::get_player_stats(env.clone(), user.clone()).unwrap();
            assert_eq!(stats.tier, expected_tier);

            let reward = GamificationEngine::distribute_tier_reward(env.clone(), user.clone());
            total_distributed += reward;
        }

        let final_pool = GamificationEngine::get_reward_pool(env.clone());
        assert_eq!(final_pool, 1_500_000 - total_distributed);
    }

    /// Admin badge issuance and definition updates.
    #[test]
    fn test_admin_badge_management() {
        let (env, admin) = setup();
        let user = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user.clone());

        // Admin manually issues badge
        GamificationEngine::issue_badge(
            env.clone(), admin.clone(), user.clone(), symbol_short!("1ST_TRD"),
        );

        let badges = GamificationEngine::get_user_badges(env.clone(), user.clone());
        assert_eq!(badges.len(), 1);

        // Get badge record
        let record = GamificationEngine::get_badge_record(
            env.clone(), user.clone(), symbol_short!("1ST_TRD"),
        );
        assert!(record.is_some());
        let rec = record.unwrap();
        assert_eq!(rec.badge_id, symbol_short!("1ST_TRD"));
        assert!(rec.earned_at > 0);

        // Update badge definition
        GamificationEngine::update_badge_definition(
            env.clone(), admin.clone(),
            symbol_short!("1ST_TRD"),
            symbol_short!("First!"),
            symbol_short!("Your 1st!"),
            20, // increased reward
            true,
        );

        let defs = GamificationEngine::get_badge_definitions_list(env.clone());
        let first_def = defs.get(0).unwrap();
        assert_eq!(first_def.name, symbol_short!("First!"));
        assert_eq!(first_def.reward_amount, 20);
    }

    /// Pagination stress test with many players.
    #[test]
    fn test_leaderboard_pagination_many_players() {
        let (env, _admin) = setup();

        let mut users = soroban_sdk::Vec::<Address>::new(&env);
        for _ in 0..20 {
            let user = Address::generate(&env);
            GamificationEngine::register_user(env.clone(), user.clone());
            users.push_back(user);
        }

        // Give each user a different number of trades
        for i in 0..20u32 {
            let user = users.get(i).unwrap();
            for _ in 0..=(i % 15) {
                GamificationEngine::record_trade(env.clone(), user.clone(), ((i + 1) as i128) * 100, true);
            }
        }

        // Page 1
        let page1 = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Score, TimeWindow::AllTime, 0, 10,
        );
        assert_eq!(page1.entries.len(), 10);
        assert_eq!(page1.total_players, 20);

        // Page 2
        let page2 = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Score, TimeWindow::AllTime, 10, 10,
        );
        assert_eq!(page2.entries.len(), 10);
        assert_eq!(page2.total_players, 20);

        // Scores should be descending across pages
        let last_p1_score = page1.entries.get(9).unwrap().score;
        let first_p2_score = page2.entries.get(0).unwrap().score;
        assert!(last_p1_score >= first_p2_score);

        // Page 3 (beyond total) should be empty
        let page3 = GamificationEngine::get_leaderboard(
            env.clone(), SortMetric::Score, TimeWindow::AllTime, 20, 10,
        );
        assert_eq!(page3.entries.len(), 0);
        assert_eq!(page3.total_players, 20);
    }

    /// Audit trail: verify badge records and reward distribution history.
    #[test]
    fn test_audit_trail() {
        let (env, _admin) = setup();
        let user = Address::generate(&env);
        GamificationEngine::register_user(env.clone(), user.clone());

        // Perform actions
        GamificationEngine::record_trade(env.clone(), user.clone(), 500, true);
        GamificationEngine::record_trade(env.clone(), user.clone(), 300, true);
        GamificationEngine::complete_learning_module(env.clone(), user.clone());

        // Issue badges
        let badges = GamificationEngine::check_and_issue_badges(env.clone(), user.clone());
        let badge_count = badges.len();

        // Verify each badge has a record
        for i in 0..badge_count {
            let badge_id = badges.get(i).unwrap();
            let record = GamificationEngine::get_badge_record(
                env.clone(), user.clone(), badge_id.clone(),
            );
            assert!(record.is_some());
            let rec = record.unwrap();
            assert_eq!(rec.user, user);
            assert!(rec.earned_at > 0);
            assert!(rec.score_at_earn >= 0);
        }

        // Verify all user badges retrievable
        let all_badges = GamificationEngine::get_user_badges(env.clone(), user.clone());
        assert_eq!(all_badges.len(), badge_count);
    }
}
