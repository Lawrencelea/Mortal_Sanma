use convlog::tenhou::Log;
use convlog::{Event, Tile, t, tenhou_to_mjai};

const SANMA_TEMPLATE: &str = include_str!("../../../template_log/template.json");
const SANMA_KITA_ALIGNMENT_LOG: &str =
    include_str!("../../../template_log/2025021522gm-00b9-0000-b5a02c99.json");
const SANMA_RON_ON_NUKIDORA_LOG: &str =
    include_str!("../../../template_log/2025020508gm-00b9-0000-79ddbd9d.json");
const SANMA_DAIMINKAN_REPLACEMENT_LOG: &str =
    include_str!("../../../template_log/2025020501gm-00b9-0000-2c6f867d.json");
const SANMA_RON_WITH_TRAILING_PHANTOM_TAKES_LOG: &str =
    include_str!("../../../template_log/2025021501gm-00b9-0000-f91fb013.json");
const SANMA_PON_AFTER_SKIPPED_TSUMOGIRI_LOG: &str =
    include_str!("../../../2025_output/2025072919gm-00b9-0000-a7cc0243.json");
// These real-log regressions summarize the stream-ordering bugs found while
// validating the 2025 sanma corpus. Each protects a branch where the replay
// scheduler must pass an earlier ron/call opportunity instead of moving future
// player-stream events ahead of intervening draws, nukidora, riichi, or discards.
const SANMA_FURITEN_BRANCH_LOG: &str =
    include_str!("../../../2025_output/2025020203gm-00b9-0000-bea67528.json");
const SANMA_RIICHI_SETUP_BRANCH_LOG: &str =
    include_str!("../../../2025_output/2025040519gm-00b9-0000-5f46d99c.json");
const SANMA_HOUTEI_BRANCH_LOG: &str =
    include_str!("../../../2025_output/2025060717gm-00b9-0000-5fafbe59.json");
const SANMA_TANYAO_RON_BRANCH_LOG: &str =
    include_str!("../../../2025_output/2025121618gm-00b9-0000-0bbd79eb.json");

fn convert_raw_log(raw: &str, description: &str) -> Vec<Event> {
    let tenhou_log =
        Log::from_json_str(raw).unwrap_or_else(|_| panic!("{description} should parse"));
    assert_eq!(tenhou_log.num_players, 3);
    tenhou_to_mjai(&tenhou_log).unwrap_or_else(|_| panic!("{description} should convert"))
}

fn convert_template() -> Vec<Event> {
    convert_raw_log(SANMA_TEMPLATE, "sanma template")
}

fn convert_kita_alignment_log() -> Vec<Event> {
    convert_raw_log(SANMA_KITA_ALIGNMENT_LOG, "sanma kita log")
}

fn convert_ron_on_nukidora_log() -> Vec<Event> {
    convert_raw_log(SANMA_RON_ON_NUKIDORA_LOG, "sanma ron-on-nukidora log")
}

fn convert_daiminkan_replacement_log() -> Vec<Event> {
    convert_raw_log(
        SANMA_DAIMINKAN_REPLACEMENT_LOG,
        "sanma daiminkan replacement log",
    )
}

fn convert_ron_with_trailing_phantom_takes_log() -> Vec<Event> {
    convert_raw_log(
        SANMA_RON_WITH_TRAILING_PHANTOM_TAKES_LOG,
        "sanma ron with trailing phantom takes log",
    )
}

fn convert_pon_after_skipped_tsumogiri_log() -> Vec<Event> {
    convert_raw_log(
        SANMA_PON_AFTER_SKIPPED_TSUMOGIRI_LOG,
        "sanma pon after skipped tsumogiri log",
    )
}

fn convert_furiten_branch_log() -> Vec<Event> {
    convert_raw_log(SANMA_FURITEN_BRANCH_LOG, "sanma furiten log")
}

fn kyoku_slice(events: &[Event], bakaze: Tile, kyoku: u8, honba: u8) -> &[Event] {
    let start = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::StartKyoku {
                    bakaze: b,
                    kyoku: k,
                    honba: h,
                    ..
                } if *b == bakaze && *k == kyoku && *h == honba
            )
        })
        .expect("target kyoku should exist");
    let end = start
        + events[start..]
            .iter()
            .position(|event| matches!(event, Event::EndKyoku))
            .expect("target kyoku should end");
    &events[start..=end]
}

fn convert_modified_first_kyoku(mut f: impl FnMut(&mut serde_json::Value)) -> Vec<Event> {
    let mut value: serde_json::Value = serde_json::from_str(SANMA_TEMPLATE).unwrap();
    let logs = value["log"].as_array_mut().unwrap();
    logs.truncate(1);
    f(&mut logs[0]);
    let tenhou_log =
        Log::from_json_str(&value.to_string()).expect("modified sanma log should parse");
    tenhou_to_mjai(&tenhou_log).expect("modified sanma log should convert")
}

fn event_tiles(event: &Event) -> Vec<Tile> {
    match event {
        Event::StartKyoku {
            dora_marker,
            tehais,
            ..
        } => {
            let mut tiles = vec![*dora_marker];
            tiles.extend(tehais.iter().flatten().copied());
            tiles
        }
        Event::Tsumo { pai, .. }
        | Event::Dahai { pai, .. }
        | Event::Nukidora { pai, .. }
        | Event::Kakan { pai, .. } => vec![*pai],
        Event::Chi { pai, consumed, .. } | Event::Pon { pai, consumed, .. } => {
            let mut tiles = vec![*pai];
            tiles.extend(consumed);
            tiles
        }
        Event::Daiminkan { pai, consumed, .. } => {
            let mut tiles = vec![*pai];
            tiles.extend(consumed);
            tiles
        }
        Event::Ankan { consumed, .. } => consumed.to_vec(),
        Event::Dora { dora_marker } => vec![*dora_marker],
        Event::Hora { ura_markers, .. } => ura_markers.clone().unwrap_or_default(),
        _ => vec![],
    }
}

#[test]
fn converts_real_sanma_template() {
    let events = convert_template();
    let kyoku_count = events
        .iter()
        .filter(|event| matches!(event, Event::StartKyoku { .. }))
        .count();
    assert_eq!(kyoku_count, 7);
    assert!(events.len() > 100);
}

#[test]
fn sanma_start_metadata_has_three_actors() {
    let events = convert_template();
    let Event::StartGame { names, .. } = &events[0] else {
        panic!("first event should be start_game");
    };
    assert_eq!(names.len(), 3);

    for event in &events {
        if let Event::StartKyoku { scores, tehais, .. } = event {
            assert_eq!(scores.len(), 3);
            assert_eq!(tehais.len(), 3);
        }
    }
}

#[test]
fn sanma_template_contains_required_action_families() {
    let events = convert_template();

    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Hora { actor, target, .. } if actor != target))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Hora { actor, target, .. } if actor == target))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Reach { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ReachAccepted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Pon { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Ankan { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Nukidora { pai, .. } if *pai == t!(N)))
    );
}

#[test]
fn converts_sanma_exhaustive_draw_result() {
    let events = convert_modified_first_kyoku(|kyoku| {
        let row = kyoku.as_array_mut().unwrap();
        *row.last_mut().unwrap() = serde_json::json!(["流局", [0, 0, 0, 0]]);
    });

    assert!(events.iter().any(|event| {
        matches!(event, Event::Ryukyoku { deltas: Some(deltas) } if deltas == &[0, 0, 0])
    }));
}

#[test]
fn sanma_template_has_no_fourth_actor_no_chi_and_no_removed_manzu() {
    let events = convert_template();

    for event in &events {
        assert!(!matches!(event, Event::Chi { .. }));
        assert!(!event.actor().is_some_and(|actor| actor == 3));

        for tile in event_tiles(event) {
            assert!(
                !matches!(tile.as_u8(), 1..=7 | 34),
                "sanma output contains removed tile {tile}"
            );
        }
    }
}

#[test]
fn sanma_initial_hand_kita_consumes_replacement_draws_before_normal_discard() {
    let events = convert_kita_alignment_log();
    let start = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::StartKyoku {
                    bakaze,
                    kyoku: 2,
                    honba: 0,
                    ..
                } if *bakaze == t!(S)
            )
        })
        .expect("S2 honba 0 should exist");
    let s2 = &events[start..];

    let expected = [
        Event::Dahai {
            actor: 1,
            pai: t!(9m),
            tsumogiri: false,
        },
        Event::Tsumo {
            actor: 2,
            pai: t!(C),
        },
        Event::Nukidora {
            actor: 2,
            pai: t!(N),
        },
        Event::Tsumo {
            actor: 2,
            pai: t!(6p),
        },
        Event::Nukidora {
            actor: 2,
            pai: t!(N),
        },
        Event::Tsumo {
            actor: 2,
            pai: t!(2p),
        },
        Event::Dahai {
            actor: 2,
            pai: t!(E),
            tsumogiri: false,
        },
        Event::Tsumo {
            actor: 0,
            pai: t!(9s),
        },
    ];

    assert!(
        s2.windows(expected.len())
            .any(|window| window == expected.as_slice()),
        "S2 should consume initial-hand kita replacement draws before actor 2's normal discard"
    );
}

#[test]
fn sanma_tsumogiri_discards_match_latest_tsumo_tile() {
    let events = convert_kita_alignment_log();
    let mut latest_tsumo = [None; 3];

    for event in events {
        match event {
            Event::Tsumo { actor, pai } => latest_tsumo[actor as usize] = Some(pai),
            Event::Dahai {
                actor,
                pai,
                tsumogiri: true,
            } => assert_eq!(
                latest_tsumo[actor as usize],
                Some(pai),
                "tsumogiri dahai should match actor {actor}'s latest tsumo"
            ),
            _ => (),
        }
    }
}

#[test]
fn sanma_nukidora_can_be_roned_without_replacement_draw() {
    let events = convert_ron_on_nukidora_log();

    let ron_on_nukidora = events.windows(3).any(|window| {
        matches!(
            &window[0],
            Event::Nukidora {
                actor: nuki_actor,
                pai,
            } if *nuki_actor == 2 && *pai == t!(N)
        ) && matches!(
            &window[1],
            Event::Hora {
                actor: 0,
                target: 2,
                ..
            }
        ) && matches!(window[2], Event::EndKyoku)
    });

    assert!(
        ron_on_nukidora,
        "nukidora should be a ron-able interrupt point like kakan"
    );
}

#[test]
fn sanma_daiminkan_replacement_tsumogiri_is_filled() {
    let events = convert_daiminkan_replacement_log();

    assert!(
        events.windows(4).any(|window| {
            matches!(
                window[0],
                Event::Daiminkan {
                    actor: 0,
                    target: 2,
                    pai,
                    ..
                } if pai == t!(9s)
            ) && matches!(
                window[1],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(E)
            ) && matches!(window[2], Event::Dora { .. })
                && matches!(
                    window[3],
                    Event::Dahai {
                        actor: 0,
                        pai,
                        tsumogiri: true,
                    } if pai == t!(E)
                )
        }),
        "daiminkan replacement draw should fill the following tsumogiri discard"
    );

    for event in events {
        if let Event::Dahai { pai, .. } = event {
            assert_ne!(pai, t!(?), "converted dahai must not contain unknown tile");
        }
    }
}

#[test]
fn sanma_ron_stops_at_targets_final_discard() {
    let events = convert_ron_with_trailing_phantom_takes_log();

    assert!(
        events.windows(3).any(|window| {
            matches!(
                window[0],
                Event::Dahai {
                    actor: 2,
                    pai,
                    ..
                } if pai == t!(8s)
            ) && matches!(
                window[1],
                Event::Hora {
                    actor: 1,
                    target: 2,
                    ..
                }
            ) && matches!(window[2], Event::EndKyoku)
        }),
        "ron should be emitted immediately after the target actor's final discard"
    );
}

#[test]
fn sanma_replay_does_not_promote_future_pon_across_nukidora() {
    let events = convert_ron_with_trailing_phantom_takes_log();
    let start = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::StartKyoku {
                    bakaze,
                    kyoku: 2,
                    honba: 1,
                    ..
                } if *bakaze == t!(S)
            )
        })
        .expect("S2 honba 1 should exist");
    let end = start
        + events[start..]
            .iter()
            .position(|event| matches!(event, Event::EndKyoku))
            .expect("S2 honba 1 should end");
    let s2_1 = &events[start..=end];

    let actor2_nuki = s2_1
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Nukidora {
                    actor: 2,
                    pai,
                } if *pai == t!(N)
            )
        })
        .expect("actor 2 should nuki before the later pon");
    let actor0_nuki = s2_1
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Nukidora {
                    actor: 0,
                    pai,
                } if *pai == t!(N)
            )
        })
        .expect("actor 0 should nuki before the later pon");
    let future_pon = s2_1
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Pon {
                    actor: 1,
                    target: 0,
                    pai,
                    consumed,
                } if *pai == t!(6s) && *consumed == [t!(6s), t!(6s)]
            )
        })
        .expect("actor 1's future pon should appear naturally");

    assert!(
        actor2_nuki < actor0_nuki && actor0_nuki < future_pon,
        "future pon must not be promoted across prior nukidora actions"
    );

    assert!(
        s2_1.windows(2).any(|window| {
            matches!(
                window[0],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: false,
                } if pai == t!(6s)
            ) && matches!(
                window[1],
                Event::Pon {
                    actor: 1,
                    target: 0,
                    pai,
                    ..
                } if pai == t!(6s)
            )
        }),
        "future pon should remain attached to the actual later 6s discard"
    );

    assert!(
        s2_1.windows(4).any(|window| {
            matches!(
                window[0],
                Event::Tsumo {
                    actor: 1,
                    pai,
                } if pai == t!(9s)
            ) && matches!(
                window[1],
                Event::Dahai {
                    actor: 1,
                    pai,
                    tsumogiri: true,
                } if pai == t!(9s)
            ) && matches!(
                window[2],
                Event::Hora {
                    actor: 2,
                    target: 1,
                    ..
                }
            ) && matches!(window[3], Event::EndKyoku)
        }),
        "the round should still reach the later actor 1 tsumogiri ron"
    );
}

#[test]
fn sanma_replay_attaches_late_stream_pon_to_earlier_discard() {
    let events = convert_pon_after_skipped_tsumogiri_log();
    let start = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::StartKyoku {
                    bakaze,
                    kyoku: 3,
                    honba: 1,
                    ..
                } if *bakaze == t!(E)
            )
        })
        .expect("E3 honba 1 should exist");
    let end = start
        + events[start..]
            .iter()
            .position(|event| matches!(event, Event::EndKyoku))
            .expect("E3 honba 1 should end");
    let e3_1 = &events[start..=end];

    assert!(
        e3_1.windows(7).any(|window| {
            matches!(
                window[0],
                Event::Dahai {
                    actor: 2,
                    pai,
                    tsumogiri: true,
                } if pai == t!(F)
            ) && matches!(
                window[1],
                Event::Pon {
                    actor: 0,
                    target: 2,
                    pai,
                    consumed,
                } if pai == t!(F) && consumed == [t!(F), t!(F)]
            ) && matches!(
                window[2],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: false,
                } if pai == t!(9m)
            ) && matches!(
                window[3],
                Event::Tsumo {
                    actor: 1,
                    pai,
                } if pai == t!(W)
            ) && matches!(
                window[4],
                Event::Dahai {
                    actor: 1,
                    pai,
                    tsumogiri: false,
                } if pai == t!(F)
            ) && matches!(window[5], Event::Ryukyoku { .. })
                && matches!(window[6], Event::EndKyoku)
        }),
        "actor 0's final pon should claim actor 2's F before actor 1's later W/F turn"
    );
}

#[test]
fn sanma_replay_rejects_passed_ron_furiten_branch() {
    // 2025020203gm-00b9-0000-bea67528: actor 2 must draw 7s and discard 9p
    // after actor 1's 9m/7p turn; scheduling it earlier creates fake furiten.
    let events = convert_furiten_branch_log();
    let s3_1 = kyoku_slice(&events, t!(S), 3, 1);

    assert!(
        s3_1.windows(7).any(|window| {
            matches!(
                window[0],
                Event::Tsumo {
                    actor: 1,
                    pai,
                } if pai == t!(9m)
            ) && matches!(
                window[1],
                Event::Dahai {
                    actor: 1,
                    pai,
                    tsumogiri: false,
                } if pai == t!(7p)
            ) && matches!(
                window[2],
                Event::Tsumo {
                    actor: 2,
                    pai,
                } if pai == t!(7s)
            ) && matches!(
                window[3],
                Event::Dahai {
                    actor: 2,
                    pai,
                    tsumogiri: false,
                } if pai == t!(9p)
            ) && matches!(
                window[4],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(4s)
            ) && matches!(
                window[5],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(4s)
            ) && matches!(
                window[6],
                Event::Hora {
                    actor: 2,
                    target: 0,
                    ..
                }
            )
        }),
        "actor 2's 7s/9p tenpai turn must not be scheduled before actor 0's earlier 1s discard"
    );
}

#[test]
fn sanma_replay_keeps_real_turn_before_riichi_branch() {
    // 2025040519gm-00b9-0000-5f46d99c: a real actor 1 6s/F turn occurs before
    // their later 3p reach. The converter must not skip it while resolving a
    // competing branch.
    let events = convert_raw_log(
        SANMA_RIICHI_SETUP_BRANCH_LOG,
        "sanma real-turn-before-riichi branch log",
    );
    let e2 = kyoku_slice(&events, t!(E), 2, 0);

    assert!(
        e2.windows(10).any(|window| {
            matches!(
                window[0],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(4s)
            ) && matches!(
                window[1],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(4s)
            ) && matches!(
                window[2],
                Event::Tsumo {
                    actor: 1,
                    pai,
                } if pai == t!(6s)
            ) && matches!(
                window[3],
                Event::Dahai {
                    actor: 1,
                    pai,
                    tsumogiri: false,
                } if pai == t!(F)
            ) && matches!(
                window[4],
                Event::Tsumo {
                    actor: 2,
                    pai,
                } if pai == t!(9m)
            ) && matches!(
                window[5],
                Event::Dahai {
                    actor: 2,
                    pai,
                    tsumogiri: true,
                } if pai == t!(9m)
            ) && matches!(
                window[6],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(8s)
            ) && matches!(
                window[7],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(8s)
            ) && matches!(
                window[8],
                Event::Tsumo {
                    actor: 1,
                    pai,
                } if pai == t!(3p)
            ) && matches!(window[9], Event::Reach { actor: 1 })
        }),
        "a real actor 1 6s/F turn must be consumed before actor 1's later riichi"
    );
}

#[test]
fn sanma_replay_waits_until_houtei_ron_branch() {
    // 2025060717gm-00b9-0000-5fafbe59: the winning ron is on actor 2's final
    // 4s discard; earlier identical call opportunities must be passed.
    let events = convert_raw_log(SANMA_HOUTEI_BRANCH_LOG, "sanma houtei branch log");
    let e1_2 = kyoku_slice(&events, t!(E), 1, 2);

    assert!(
        e1_2.windows(7).any(|window| {
            matches!(
                window[0],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(P)
            ) && matches!(
                window[1],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(P)
            ) && matches!(
                window[2],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(1p)
            ) && matches!(
                window[3],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(1p)
            ) && matches!(
                window[4],
                Event::Tsumo {
                    actor: 2,
                    pai,
                } if pai == t!(6p)
            ) && matches!(
                window[5],
                Event::Dahai {
                    actor: 2,
                    pai,
                    tsumogiri: false,
                } if pai == t!(4s)
            ) && matches!(
                window[6],
                Event::Hora {
                    actor: 1,
                    target: 2,
                    ..
                }
            )
        }),
        "houtei ron branch should not end before the final live-wall discard"
    );
}

#[test]
fn sanma_replay_keeps_terminal_cleanup_before_tanyao_ron() {
    // 2025121618gm-00b9-0000-0bbd79eb: actor 2 clears a terminal with 4s/1s
    // and actor 0 has another W/W turn before actor 1's 2s ron discard.
    let events = convert_raw_log(SANMA_TANYAO_RON_BRANCH_LOG, "sanma tanyao ron branch log");
    let e1 = kyoku_slice(&events, t!(E), 1, 0);

    assert!(
        e1.windows(11).any(|window| {
            matches!(
                window[0],
                Event::Tsumo {
                    actor: 2,
                    pai,
                } if pai == t!(1m)
            ) && matches!(
                window[1],
                Event::Dahai {
                    actor: 2,
                    pai,
                    tsumogiri: true,
                } if pai == t!(1m)
            ) && matches!(
                window[2],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(W)
            ) && matches!(
                window[3],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(W)
            ) && matches!(
                window[4],
                Event::Tsumo {
                    actor: 2,
                    pai,
                } if pai == t!(4s)
            ) && matches!(
                window[5],
                Event::Dahai {
                    actor: 2,
                    pai,
                    tsumogiri: false,
                } if pai == t!(1s)
            ) && matches!(
                window[6],
                Event::Tsumo {
                    actor: 0,
                    pai,
                } if pai == t!(W)
            ) && matches!(
                window[7],
                Event::Dahai {
                    actor: 0,
                    pai,
                    tsumogiri: true,
                } if pai == t!(W)
            ) && matches!(
                window[8],
                Event::Tsumo {
                    actor: 1,
                    pai,
                } if pai == t!(2s)
            ) && matches!(
                window[9],
                Event::Dahai {
                    actor: 1,
                    pai,
                    tsumogiri: true,
                } if pai == t!(2s)
            ) && matches!(
                window[10],
                Event::Hora {
                    actor: 2,
                    target: 1,
                    ..
                }
            )
        }),
        "tanyao ron branch should keep actor 2's real terminal-clearing draw/discard"
    );
}
