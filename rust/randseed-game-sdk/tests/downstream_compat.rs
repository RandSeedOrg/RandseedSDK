use randseed_game_sdk::{PlatformClient, PlayerInfo, PlayerProfileInfo, TickContext, TickRequest};
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

#[test]
fn legacy_player_info_literal_and_roster_method_remain_available() {
    let legacy_player = PlayerInfo {
        player_id: "seat-a".to_string(),
        joined_at: 1,
    };
    assert_eq!(legacy_player.player_id, "seat-a");
    assert_eq!(legacy_player.joined_at, 1);

    let profile_player = PlayerProfileInfo {
        player_id: "seat-a".to_string(),
        joined_at: 1,
        display_name: None,
        avatar_url: None,
        is_game_creator: Some(false),
    };
    assert_eq!(profile_player.is_game_creator, Some(false));

    let client = PlatformClient::new();
    drop(client.get_players());
    drop(client.get_players_with_profiles());
}
