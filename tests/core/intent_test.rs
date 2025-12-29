//! Comprehensive tests for the ivza-core intent module.
//!
//! Tests cover: IntentParser (JSON and DSL), IntentResolver (swap, multi-hop,
//! stake, transfer, unstake, provide-liquidity, create-account).

use ivza_core::intent::{
    Intent, IntentParams, IntentParser, IntentResolver, IntentType, MultiHopSwapParams,
    StakeParams, SwapParams, TransferParams,
};
use solana_sdk::pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn pubkey_to_string(p: &Pubkey) -> String {
    p.to_string()
}

// ---------------------------------------------------------------------------
// IntentParser JSON tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_swap_json() {
    let wallet = make_pubkey(1);
    let input_mint = make_pubkey(2);
    let output_mint = make_pubkey(3);

    let json = format!(
        r#"{{
            "type": "swap",
            "params": {{
                "input_mint": "{}",
                "output_mint": "{}",
                "amount_in": 1000000,
                "user_wallet": "{}"
            }},
            "label": "test_swap",
            "max_slippage_bps": 100
        }}"#,
        pubkey_to_string(&input_mint),
        pubkey_to_string(&output_mint),
        pubkey_to_string(&wallet),
    );

    let parser = IntentParser::new();
    let intent = parser.parse_json(&json).unwrap();

    assert_eq!(intent.intent_type, IntentType::Swap);
    assert_eq!(intent.label.as_deref(), Some("test_swap"));
    assert_eq!(intent.max_slippage_bps, Some(100));

    if let IntentParams::Swap(params) = &intent.params {
        assert_eq!(params.input_mint, input_mint);
        assert_eq!(params.output_mint, output_mint);
        assert_eq!(params.amount_in, 1_000_000);
        assert_eq!(params.user_wallet, wallet);
    } else {
        panic!("Expected Swap params");
    }
}

#[test]
fn test_parse_multi_hop_json() {
    let wallet = make_pubkey(1);
    let route: Vec<Pubkey> = (2..=5).map(|i| make_pubkey(i)).collect();

    let route_strs: Vec<String> = route.iter().map(|p| format!("\"{}\"", p)).collect();
    let json = format!(
        r#"{{
            "type": "multi_hop_swap",
            "params": {{
                "route": [{}],
                "amount_in": 500000,
                "user_wallet": "{}"
            }}
        }}"#,
        route_strs.join(", "),
        pubkey_to_string(&wallet),
    );

    let parser = IntentParser::new();
    let intent = parser.parse_json(&json).unwrap();

    assert_eq!(intent.intent_type, IntentType::MultiHopSwap);
    if let IntentParams::MultiHopSwap(params) = &intent.params {
        assert_eq!(params.route.len(), 4);
        assert_eq!(params.amount_in, 500_000);
    } else {
        panic!("Expected MultiHopSwap params");
    }
}

#[test]
fn test_parse_stake_json() {
    let wallet = make_pubkey(1);
    let validator = make_pubkey(10);

    let json = format!(
        r#"{{
            "type": "stake",
            "params": {{
                "amount": 2000000000,
                "validator_vote_account": "{}",
                "user_wallet": "{}"
            }}
        }}"#,
        pubkey_to_string(&validator),
        pubkey_to_string(&wallet),
    );

    let parser = IntentParser::new();
    let intent = parser.parse_json(&json).unwrap();

    assert_eq!(intent.intent_type, IntentType::Stake);
    if let IntentParams::Stake(params) = &intent.params {
        assert_eq!(params.amount, 2_000_000_000);
        assert_eq!(params.validator_vote_account, validator);
    } else {
        panic!("Expected Stake params");
    }
}

#[test]
fn test_parse_transfer_json() {
    let from = make_pubkey(1);
    let to = make_pubkey(2);
    let mint = make_pubkey(3);

    let json = format!(
        r#"{{
            "type": "transfer",
            "params": {{
                "mint": "{}",
                "amount": 100000,
                "from_wallet": "{}",
                "to_wallet": "{}"
            }}
        }}"#,
        pubkey_to_string(&mint),
        pubkey_to_string(&from),
        pubkey_to_string(&to),
    );

    let parser = IntentParser::new();
    let intent = parser.parse_json(&json).unwrap();

    assert_eq!(intent.intent_type, IntentType::Transfer);
}

#[test]
fn test_parse_json_invalid() {
    let parser = IntentParser::new();
    assert!(parser.parse_json("not json").is_err());
}

#[test]
fn test_parse_json_missing_type() {
    let parser = IntentParser::new();
    let json = r#"{"params": {}}"#;
    assert!(parser.parse_json(json).is_err());
}

#[test]
fn test_parse_json_unknown_type() {
    let parser = IntentParser::new();
    let json = r#"{"type": "foobar", "params": {}}"#;
    assert!(parser.parse_json(json).is_err());
}

#[test]
fn test_parse_batch_json() {
    let wallet = make_pubkey(1);
    let input_mint = make_pubkey(2);
    let output_mint = make_pubkey(3);

    let json = format!(
        r#"[
            {{
                "type": "swap",
                "params": {{
                    "input_mint": "{}",
                    "output_mint": "{}",
                    "amount_in": 1000,
                    "user_wallet": "{}"
                }}
            }},
            {{
                "type": "stake",
                "params": {{
                    "amount": 2000,
                    "validator_vote_account": "{}",
                    "user_wallet": "{}"
                }}
            }}
        ]"#,
        input_mint, output_mint, wallet, make_pubkey(10), wallet,
    );

    let parser = IntentParser::new();
    let intents = parser.parse_batch(&json).unwrap();

    assert_eq!(intents.len(), 2);
    assert_eq!(intents[0].intent_type, IntentType::Swap);
    assert_eq!(intents[1].intent_type, IntentType::Stake);
}

#[test]
fn test_parse_json_with_priority_fee() {
    let wallet = make_pubkey(1);
    let json = format!(
        r#"{{
            "type": "stake",
            "params": {{
                "amount": 1000,
                "validator_vote_account": "{}",
                "user_wallet": "{}"
            }},
            "priority_fee": 5000
        }}"#,
        make_pubkey(10),
        wallet,
    );

    let parser = IntentParser::new();
    let intent = parser.parse_json(&json).unwrap();
    assert_eq!(intent.priority_fee, Some(5000));
}

// ---------------------------------------------------------------------------
// IntentParser DSL tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_swap_dsl() {
    let input_mint = make_pubkey(2);
    let output_mint = make_pubkey(3);
    let wallet = make_pubkey(1);

    let dsl = format!(
        "swap 1000000 {} for {} by {}",
        input_mint, output_mint, wallet,
    );

    let parser = IntentParser::new();
    let intent = parser.parse_dsl(&dsl).unwrap();

    assert_eq!(intent.intent_type, IntentType::Swap);
    if let IntentParams::Swap(params) = &intent.params {
        assert_eq!(params.amount_in, 1_000_000);
        assert_eq!(params.input_mint, input_mint);
        assert_eq!(params.output_mint, output_mint);
    } else {
        panic!("Expected Swap params");
    }
}
