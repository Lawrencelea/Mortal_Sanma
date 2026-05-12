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
const SANMA_CFB86147_KYOKU_JSON: &str = r#"[[2,0,0],[29700,30400,44900,0],[26],[],[24,25,28,28,32,34,36,37,39,42,45,47,47],[41,34,45,22,53,35,19,47,37,"37p3737",24,"45p4545"],[39,41,42,60,28,28,32,19,25,24,60,36],[11,21,22,22,23,23,52,27,28,38,39,42,46],[27,36,38,25,26,43,43,11,19,23,19],[11,39,42,46,36,60,60,60,60,60,60],[11,19,22,26,29,34,35,36,39,42,42,43,44],[43,46,29,37,"p424242",35,31,"p434343",31,28,37,45,44,45],["f44",19,11,46,22,26,29,29,39,60,60,60,"f44",60],[],[],[],["和了",[-11600,0,11600,0],[2,0,2,"30符4飜11600点","混一色(2飜)","ドラ(2飜)"]]]"#;
const SANMA_097ACC2B_KYOKU_JSON: &str = r#"[[2,0,0],[25500,33000,46500,0],[35],[39],[21,22,22,23,23,26,28,29,32,34,42,45,45],[25,37,34,32,"45p4545",27],[34,32,60,60,42,37],[23,24,24,25,26,29,34,36,38,42,43,47,47],[53,31,39,25,32],[42,60,43,"r29",60],[22,27,29,32,33,35,36,37,38,39,42,43,45],[28,31,43,46,45,36],[43,42,60,45,60,46],[],[],[],["和了",[-5200,6200,0,0],[1,0,1,"40符3飜5200点","立直(1飜)","ドラ(1飜)","赤ドラ(1飜)"]]]"#;
const SANMA_25E9BC69_KYOKU_JSON: &str = r#"[[1,1,0],[51700,25900,27400,0],[42],[28],[11,11,21,26,29,29,37,41,42,42,43,45,45],["2929p29","11p1111",44,32,"4545p45",27,19,47,34],[26,37,"f44",60,21,60,60,60,42],[19,19,22,22,23,24,29,53,36,36,45,46,46],[25,39,28,35,33,34,37,29,21],[29,60,60,45,60,22,"r22",60,60],[11,22,23,23,25,26,26,33,38,38,41,47,47],[28,52,31,36,26,32,19],[11,28,33,60,22,60,60],[],[],[],["和了",[0,5100,-4100,0],[1,2,1,"40符2飜3900点","立直(1飜)","赤ドラ(1飜)"]]]"#;
const SANMA_6C887319_KYOKU_JSON: &str = r#"[[0,2,0],[47200,35000,22800,0],[23],[44],[21,21,22,26,32,33,33,34,35,38,43,43,46],[22,42,46,29,38,35,28,32],[46,60,60,60,26,34,"r32",60],[11,19,19,22,23,26,26,31,33,34,36,37,43],[39,27,35,25,19,28],[43,31,39,26,60,60],[22,24,24,28,29,29,53,36,36,41,46,46,47],["46p4646",24,45,"29p2929",39,41,26],[41,47,60,28,60,60,60],[],[],[],["和了",[6200,-5200,0,0],[0,1,0,"25符3飜4800点","立直(1飜)","七対子(2飜)"]]]"#;
const SANMA_525BE759_KAKAN_BRANCH_KYOKU_JSON: &str = r#"[[1,2,0],[34000,42900,28100,0],[29,22],[],[21,26,27,29,29,31,32,33,33,35,37,39,43],[53,33,32,25,24,19,38,45],[43,29,29,21,27,60,33,60],[19,26,29,31,32,32,34,37,41,41,42,42,45],[34,21,28,"3434p34",27,"4141p41",36,37,34,46,39,42],[26,29,60,21,60,19,45,60,"3434k3434",60,60,31],[11,11,21,21,22,23,28,33,43,45,46,47,47],[22,28,34,36,41,35,52,41,28,36],[43,33,60,60,60,60,45,60,46,60],[],[],[],["和了",[4300,-4300,0,0],[0,1,0,"30符3飜3900点","平和(1飜)","一盃口(1飜)","赤ドラ(1飜)"]]]"#;
// Actor 0 (dealer) waits tanki on 1s with 中 koutsu.  Actor 1 draws and
// tsumogiri 1s (the real ron discard).  Actor 2 is a bystander whose Tenhou
// stream records a phantom 1s draw+tsumogiri AFTER the game ended.  Without
// the fix, the scheduler schedules actor 2's phantom 1s discard first, which
// sets temporary_furiten on actor 0, and the eventual hora fails validation
// with "furiten: true".
const SANMA_525BE759_BYSTANDER_PHANTOM_KYOKU_JSON: &str = r#"[[1,0,0],[35000,35000,35000,0],[47],[],[25,26,27,32,33,34,36,36,36,47,47,47,31],[38],[60],[11,19,29,41,41,42,42,43,43,45,45,46,46],[31],[60],[21,22,23,24,28,29,35,37,38,39,44,45,46],[31],[60],[],[],[],["和了",[5200,-5200,0,0],[0,1,0,"30符3飜5200点","中(1飜)","ドラ(2飜)"]]]"#;

fn convert_raw_log(raw: &str, description: &str) -> Vec<Event> {
    let tenhou_log =
        Log::from_json_str(raw).unwrap_or_else(|err| panic!("{description} should parse: {err}"));
    assert_eq!(tenhou_log.num_players, 3);
    tenhou_to_mjai(&tenhou_log).unwrap_or_else(|err| panic!("{description} should convert: {err}"))
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

fn convert_single_kyoku_json(kyoku_json: &str, description: &str) -> Vec<Event> {
    let mut value: serde_json::Value = serde_json::from_str(SANMA_TEMPLATE).unwrap();
    value["log"] = serde_json::json!([serde_json::from_str::<serde_json::Value>(kyoku_json)
        .unwrap_or_else(|_| panic!("{description} kyoku json should parse"))]);
    convert_raw_log(&value.to_string(), description)
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
    // The correct branch reaches houtei (live-wall empty) with actor 0 drawing
    // both P and 1p (tsumogiri) before the final tsumo(2,6p)/dahai(2,4s)/hora.
    let events = convert_raw_log(SANMA_HOUTEI_BRANCH_LOG, "sanma houtei branch log");
    let e1_2 = kyoku_slice(&events, t!(E), 1, 2);

    // Final three events: actor 2 draws 6p, discards 4s, actor 1 rons.
    assert!(
        e1_2.windows(3).any(|window| {
            matches!(window[0], Event::Tsumo { actor: 2, pai } if pai == t!(6p))
                && matches!(
                    window[1],
                    Event::Dahai {
                        actor: 2,
                        pai,
                        tsumogiri: false,
                    } if pai == t!(4s)
                )
                && matches!(
                    window[2],
                    Event::Hora {
                        actor: 1,
                        target: 2,
                        ..
                    }
                )
        }),
        "houtei ron branch should not end before the final live-wall discard"
    );

    // Actor 0's two tsumogiri turns (P and 1p) must both appear before the hora.
    let hora_pos = e1_2
        .iter()
        .rposition(|e| {
            matches!(
                e,
                Event::Hora {
                    actor: 1,
                    target: 2,
                    ..
                }
            )
        })
        .expect("hora should exist");
    let pre_hora = &e1_2[..hora_pos];
    assert!(
        pre_hora
            .iter()
            .any(|e| matches!(e, Event::Dahai { actor: 0, pai, tsumogiri: true } if *pai == t!(P))),
        "actor 0 must tsumogiri P before houtei hora"
    );
    assert!(
        pre_hora.iter().any(
            |e| matches!(e, Event::Dahai { actor: 0, pai, tsumogiri: true } if *pai == t!(1p))
        ),
        "actor 0 must tsumogiri 1p before houtei hora"
    );
}

#[test]
fn sanma_replay_keeps_terminal_cleanup_before_tanyao_ron() {
    // 2025121618gm-00b9-0000-0bbd79eb: actor 2 clears a terminal with 4s/1s
    // (non-tsumogiri discard) before actor 1's 2s ron discard.
    // The converter must not omit this real actor-2 turn or reorder it after
    // the ron moment.
    let events = convert_raw_log(SANMA_TANYAO_RON_BRANCH_LOG, "sanma tanyao ron branch log");
    let e1 = kyoku_slice(&events, t!(E), 1, 0);

    // Actor 2's 4s/1s terminal cleanup leads directly into the final ron sequence:
    // tsumo(2,4s), dahai(2,1s,false), <actor 0 tsumogiri turn>, tsumo(1,2s),
    // dahai(1,2s,true), hora(2,target:1).
    assert!(
        e1.windows(7).any(|window| {
            matches!(window[0], Event::Tsumo { actor: 2, pai } if pai == t!(4s))
                && matches!(
                    window[1],
                    Event::Dahai {
                        actor: 2,
                        pai,
                        tsumogiri: false,
                    } if pai == t!(1s)
                )
                && matches!(window[2], Event::Tsumo { actor: 0, .. })
                && matches!(
                    window[3],
                    Event::Dahai {
                        actor: 0,
                        tsumogiri: true,
                        ..
                    }
                )
                && matches!(window[4], Event::Tsumo { actor: 1, pai } if pai == t!(2s))
                && matches!(
                    window[5],
                    Event::Dahai {
                        actor: 1,
                        pai,
                        tsumogiri: true,
                    } if pai == t!(2s)
                )
                && matches!(
                    window[6],
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

#[test]
fn sanma_replay_converts_cfb86147_pon_branch() {
    // 2020041019gm-00b9-0000-cfb86147 E3: the winner's stream contains two
    // pon calls after nuki-dora entries. A wrong scheduler branch used to end
    // with `unexpected naki ... expected tile P from Some(2)`.
    let events =
        convert_single_kyoku_json(SANMA_CFB86147_KYOKU_JSON, "sanma cfb86147 pon branch log");
    let s3 = kyoku_slice(&events, t!(E), 3, 0);

    assert!(
        s3.iter().any(|event| matches!(
            event,
            Event::Hora {
                actor: 2,
                target: 0,
                ..
            }
        )),
        "cfb86147 branch should reach actor 2's ron on actor 0"
    );
    assert!(
        s3.iter().any(
            |event| matches!(event, Event::Pon { actor: 0, target: 2, pai, .. } if *pai == t!(P))
        ),
        "actor 0's late P pon must remain attached to actor 2's discard"
    );
}

#[test]
fn sanma_replay_defers_ambiguous_pon_until_after_riichi() {
    // 2020100401gm-00b9-0000-097acc2b E3: actor 0 has a P pon in their stream,
    // and actor 2 discards P both before and after actor 1's riichi. The real
    // pon is the later one; moving it before riichi incorrectly preserves
    // ippatsu and makes the validator score the ron as 8000 instead of 5200.
    let events =
        convert_single_kyoku_json(SANMA_097ACC2B_KYOKU_JSON, "sanma 097acc2b pon branch log");
    let e3 = kyoku_slice(&events, t!(E), 3, 0);

    let reach_accepted = e3
        .iter()
        .position(|event| matches!(event, Event::ReachAccepted { actor: 1 }))
        .expect("actor 1 riichi should be accepted");
    let late_pon = e3
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Pon {
                    actor: 0,
                    target: 2,
                    pai,
                    ..
                } if *pai == t!(P)
            )
        })
        .expect("actor 0 should pon actor 2's P");
    let hora = e3
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Hora {
                    actor: 1,
                    target: 0,
                    ..
                }
            )
        })
        .expect("actor 1 should ron actor 0");

    assert!(
        reach_accepted < late_pon && late_pon < hora,
        "ambiguous P pon should be attached to the post-riichi discard"
    );
    assert!(
        matches!(
            e3.get(late_pon - 1),
            Some(Event::Dahai {
                actor: 2,
                pai,
                tsumogiri: true,
            }) if *pai == t!(P)
        ),
        "late pon should claim actor 2's post-riichi P tsumogiri"
    );
}

#[test]
fn sanma_replay_converts_no_ippatsu_counterexamples() {
    for (kyoku_json, description, actor, target) in [
        (
            SANMA_25E9BC69_KYOKU_JSON,
            "sanma 25e9bc69 no-ippatsu ron log",
            1,
            2,
        ),
        (
            SANMA_6C887319_KYOKU_JSON,
            "sanma 6c887319 no-ippatsu ron log",
            0,
            1,
        ),
    ] {
        let events = convert_single_kyoku_json(kyoku_json, description);
        let hora = events
            .iter()
            .find(|event| matches!(event, Event::Hora { actor: a, target: t, .. } if *a == actor && *t == target))
            .expect("target ron should convert");

        assert!(
            matches!(
                hora,
                Event::Hora {
                    deltas: Some(_),
                    ..
                }
            ),
            "{description} should preserve Tenhou deltas"
        );
    }
}

#[test]
fn sanma_bystander_phantom_discard_does_not_set_furiten_before_ron() {
    // 525be759 bug: actor 2 is a bystander whose Tenhou stream has a phantom
    // 1s draw+tsumogiri recorded after the game ends.  If the scheduler
    // processes actor 2's 1s discard before actor 1's real 1s discard, it
    // sets temporary_furiten on actor 0 (who is waiting for 1s).  The fix
    // deprioritises bystander candidates whose discard tile equals the ron
    // winning tile when the real target still has that tile in their stream.
    let events = convert_single_kyoku_json(
        SANMA_525BE759_BYSTANDER_PHANTOM_KYOKU_JSON,
        "sanma 525be759 bystander phantom furiten log",
    );

    let hora_pos = events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::Hora {
                    actor: 0,
                    target: 1,
                    ..
                }
            )
        })
        .expect("actor 0 should ron actor 1's 1s");

    // Actor 2's phantom 1s discard must NOT appear before the hora.
    assert!(
        !events[..hora_pos]
            .iter()
            .any(|e| matches!(e, Event::Dahai { actor: 2, pai, .. } if *pai == t!(1s))),
        "bystander actor 2's phantom 1s discard must not precede actor 0's ron"
    );
}

#[test]
fn sanma_passed_kakan_ron_branch_is_deprioritized() {
    // 2024100923gm-00b9-0000-525be759: if actor 0 draws 8s before actor 1's
    // 4s kakan, actor 0 can rob that kan.  Since Tenhou's result is a later
    // ordinary ron on actor 1's 1s discard, that chronology is the wrong
    // branch; the kakan must be scheduled before actor 0's 8s draw.
    let events = convert_single_kyoku_json(
        SANMA_525BE759_KAKAN_BRANCH_KYOKU_JSON,
        "sanma 525be759 kakan branch log",
    );

    let kakan_pos = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Kakan {
                    actor: 1,
                    pai,
                    ..
                } if *pai == t!(4s)
            )
        })
        .expect("actor 1 should kakan 4s");
    let actor0_8s_pos = events
        .iter()
        .position(|event| matches!(event, Event::Tsumo { actor: 0, pai } if *pai == t!(8s)))
        .expect("actor 0 should draw 8s");
    let hora_pos = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Hora {
                    actor: 0,
                    target: 1,
                    ..
                }
            )
        })
        .expect("actor 0 should ron actor 1");

    assert!(
        kakan_pos < actor0_8s_pos,
        "actor 1's kakan must precede actor 0's 8s draw"
    );
    assert!(
        actor0_8s_pos < hora_pos,
        "actor 0's 8s draw is a real turn before the final ron"
    );
}
