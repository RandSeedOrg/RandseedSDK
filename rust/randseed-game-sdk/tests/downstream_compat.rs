use randseed_game_sdk::{TickContext, TickRequest};
use serde_json::json;

// Integration tests compile as a downstream crate and preserve the pre-existing
// public `Vec<String>` slice-access source contract.
#[test]
fn tick_request_keeps_public_vec_roster_api() {
    let request = TickRequest {
        ctx: TickContext {
            tick: 1,
            elapsed_seconds: 0.1,
            inputs: Vec::new(),
            commands: Vec::new(),
        },
        state: json!({}),
        players: vec!["seat-a".to_string()],
    };
    let players: &[String] = request.players.as_slice();
    assert_eq!(players, &["seat-a".to_string()]);

    let mut empty_request = request;
    empty_request.players.clear();
    let legacy_wire = serde_json::to_value(&empty_request).unwrap();
    assert!(legacy_wire.get("players").is_none());
    let explicit_wire = serde_json::to_value(empty_request.with_explicit_players()).unwrap();
    assert_eq!(explicit_wire["players"], json!([]));
}
