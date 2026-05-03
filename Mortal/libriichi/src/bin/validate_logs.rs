use libriichi::hand::tiles_to_string;
use libriichi::mjai::Event;
use libriichi::state::{ActionCandidate, PlayerState};
use libriichi::tile::Tile;
use std::convert::identity;
use std::env;
use std::fs::File;
use std::io;
use std::panic::catch_unwind;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use flate2::read::GzDecoder;
use glob::glob;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde_json as json;

const USAGE: &str = "Usage: validate_logs <DIR>";
const SANMA_SCORE_TOLERANCE: i32 = 500;

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let dir = args.get(1).context(USAGE)?;

    const TEMPLATE: &str = "{spinner:.cyan} [{elapsed_precise}] {pos} ({per_sec})";
    let bar = ProgressBar::new_spinner()
        .with_style(ProgressStyle::with_template(TEMPLATE)?.tick_chars(".oO°Oo*"));
    bar.enable_steady_tick(Duration::from_millis(150));

    glob(&format!("{dir}/**/*.json"))?
        .chain(glob(&format!("{dir}/**/*.json.gz"))?)
        .chain(glob(&format!("{dir}/**/*.jsonl"))?)
        .chain(glob(&format!("{dir}/**/*.jsonl.gz"))?)
        .par_bridge()
        .try_for_each(|path| {
            bar.inc(1);
            let path = path?;

            let result = catch_unwind(|| process_path(&path))
                .map_err(|pnc| {
                    if let Some(v) = pnc.downcast_ref::<String>() {
                        anyhow!("{v}")
                    } else if let Some(v) = pnc.downcast_ref::<&str>() {
                        anyhow!("{v}")
                    } else {
                        anyhow!("Non-string panic")
                    }
                })
                .and_then(identity)
                .with_context(|| format!("error in log {}", path.display()));
            if let Err(err) = result {
                println!("\n{err:?}");
            }

            anyhow::Ok(())
        })?;

    bar.abandon();

    Ok(())
}

fn process_path(path: &Path) -> Result<()> {
    let trace_filter = env::var("VALIDATE_TRACE").ok();
    let trace_actor = env::var("VALIDATE_TRACE_ACTOR")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|actor| *actor < 3);
    let should_trace = trace_filter.as_ref().is_some_and(|filter| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(filter))
    });

    let raw_log = if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("gz"))
    {
        io::read_to_string(GzDecoder::new(File::open(path)?))?
    } else {
        io::read_to_string(File::open(path)?)?
    };
    let events: Vec<Event> = raw_log
        .lines()
        .map(|l| Ok(json::from_str(l)?))
        .collect::<Result<_>>()?;
    validate_events_impl(&events, should_trace, trace_actor)
}

#[cfg(test)]
fn validate_events(events: &[Event]) -> Result<()> {
    validate_events_impl(events, false, None)
}

fn validate_events_impl(
    events: &[Event],
    should_trace: bool,
    trace_actor: Option<usize>,
) -> Result<()> {
    let mut states = [
        PlayerState::new(0),
        PlayerState::new(1),
        PlayerState::new(2),
    ];
    let mut cans = [ActionCandidate::default(); 3];

    for (idx, ev) in events.iter().enumerate() {
        let line = idx + 1;
        match ev {
            Event::Dahai { actor, pai, .. } => {
                ensure!(
                    cans[*actor as usize].can_discard,
                    "fails can_discard at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );

                let discard_candidates = states[*actor as usize].discard_candidates_aka();
                ensure!(
                    discard_candidates[pai.as_usize()],
                    "fails discard_candidates at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );
            }
            Event::Chi { .. } => {
                anyhow::bail!("sanma log contains chi at line {line}: {ev:?}");
            }
            Event::Pon { actor, .. } => {
                ensure!(
                    cans[*actor as usize].can_pon,
                    "fails can_pon at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );
            }
            Event::Daiminkan { actor, .. } => {
                ensure!(
                    cans[*actor as usize].can_daiminkan,
                    "fails can_daiminkan at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );
            }
            Event::Ankan { actor, consumed } => {
                ensure!(
                    cans[*actor as usize].can_ankan,
                    "fails can_ankan at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );

                let ankan_candidates = states[*actor as usize].ankan_candidates();
                ensure!(
                    ankan_candidates.contains(&consumed[0].deaka()),
                    "fails ankan_candidates at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );
            }
            Event::Kakan { actor, pai, .. } => {
                ensure!(
                    cans[*actor as usize].can_kakan,
                    "fails can_kakan at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );

                let kakan_candidates = states[*actor as usize].kakan_candidates();
                ensure!(
                    kakan_candidates.contains(&pai.deaka()),
                    "fails kakan_candidates at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );
            }
            Event::Reach { actor } => {
                ensure!(
                    cans[*actor as usize].can_riichi,
                    "fails can_riichi at line {line}\naction: {ev:?}\nstate:\n{}",
                    states[*actor as usize].brief_info(),
                );
            }
            Event::Hora {
                actor,
                target,
                ura_markers,
                deltas,
            } => {
                let is_ron = actor != target;
                if is_ron {
                    ensure!(
                        cans[*actor as usize].can_ron_agari,
                        "fails can_ron_agari at line {line}\naction: {ev:?}\nstate:\n{}",
                        states[*actor as usize].brief_info(),
                    );
                } else {
                    ensure!(
                        cans[*actor as usize].can_tsumo_agari,
                        "fails can_tsumo_agari at line {line}\naction: {ev:?}\nstate:\n{}",
                        states[*actor as usize].brief_info(),
                    );
                }

                // This is a rough test
                // TODO: fix bug for double chankan ron
                let ura = ura_markers
                    .as_ref()
                    .context("missing field `ura_markers`")?;
                let deltas = deltas.as_ref().context("missing field `deltas`")?;
                let points = states[*actor as usize]
                    .agari_points(is_ron, ura)
                    .with_context(|| {
                        format!(
                            "failed to get agari points at line {line}\naction: {ev:?}\nstate:\n{}",
                            states[*actor as usize].brief_info()
                        )
                    })?;

                if is_ron {
                    ensure!(
                        deltas[*actor as usize] + SANMA_SCORE_TOLERANCE >= points.ron,
                        "fails ron score check at line {line}: delta {} < ron {}\naction: {ev:?}\nstate:\n{}",
                        deltas[*actor as usize],
                        points.ron,
                        states[*actor as usize].brief_info(),
                    );
                } else if states[*actor as usize].is_oya() {
                    ensure!(
                        deltas[*actor as usize] + SANMA_SCORE_TOLERANCE >= points.tsumo_oya,
                        "fails oya tsumo score check at line {line}: delta {} < tsumo_oya {}\naction: {ev:?}\nstate:\n{}",
                        deltas[*actor as usize],
                        points.tsumo_oya,
                        states[*actor as usize].brief_info(),
                    );
                } else {
                    ensure!(
                        deltas[*actor as usize] + SANMA_SCORE_TOLERANCE >= points.tsumo_ko,
                        "fails ko tsumo score check at line {line}: delta {} < tsumo_ko {}\naction: {ev:?}\nstate:\n{}",
                        deltas[*actor as usize],
                        points.tsumo_ko,
                        states[*actor as usize].brief_info(),
                    );
                }
            }
            _ => (),
        }

        for (s, c) in states.iter_mut().zip(&mut cans) {
            *c = s.update_with_keep_cans(ev, true)?;
        }

        if should_trace {
            if let Some(actor) = trace_actor {
                print_trace_state(line, ev, actor, &states[actor], &cans[actor]);
            } else {
                for actor in 0..3 {
                    print_trace_state(line, ev, actor, &states[actor], &cans[actor]);
                }
            }
        }
    }

    Ok(())
}

fn print_trace_state(
    line: usize,
    ev: &Event,
    actor: usize,
    state: &PlayerState,
    cans: &ActionCandidate,
) {
    let waits = state
        .waits()
        .iter()
        .enumerate()
        .filter(|&(_, &is_wait)| is_wait)
        .filter_map(|(idx, _)| Tile::try_from(idx).ok())
        .map(|tile| tile.to_string())
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "TRACE line={line} actor={actor} furiten={} shanten={} tehai=\"{}\" waits=[{}] cans={:?} ev={:?}",
        state.at_furiten(),
        state.shanten(),
        tiles_to_string(&state.tehai(), state.akas_in_hand()),
        waits,
        cans,
        ev,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use libriichi::t;

    // Fixture from 2025121618gm-00b9-0000-0bbd79eb E1. This is the compact
    // validator-side version of the terminal-cleanup edge case: actor 2 must
    // draw 4s and discard 1s before actor 1's final 2s ron discard.
    const TANYAO_RON_VALID_JSONL: &str = r#"
{"type":"start_game","names":["Cabs","Settsu","Ryuhei"]}
{"type":"start_kyoku","bakaze":"E","dora_marker":"7s","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[35000,35000,35000],"tehais":[["9m","9m","3p","5p","7p","3s","6s","7s","9s","E","N","P","P"],["1p","1p","3p","4p","5p","6p","9p","3s","9s","E","W","F","C"],["1m","9m","2p","3p","4p","8p","1s","3s","4s","5s","9s","E","C"]]}
{"type":"tsumo","actor":0,"pai":"F"}
{"type":"nukidora","actor":0,"pai":"N"}
{"type":"tsumo","actor":0,"pai":"1m"}
{"type":"dahai","actor":0,"pai":"1m","tsumogiri":true}
{"type":"tsumo","actor":1,"pai":"2p"}
{"type":"dahai","actor":1,"pai":"W","tsumogiri":false}
{"type":"tsumo","actor":2,"pai":"8s"}
{"type":"dahai","actor":2,"pai":"1m","tsumogiri":false}
{"type":"tsumo","actor":0,"pai":"N"}
{"type":"nukidora","actor":0,"pai":"N"}
{"type":"tsumo","actor":0,"pai":"F"}
{"type":"dahai","actor":0,"pai":"9s","tsumogiri":false}
{"type":"tsumo","actor":1,"pai":"7p"}
{"type":"dahai","actor":1,"pai":"9s","tsumogiri":false}
{"type":"tsumo","actor":2,"pai":"8s"}
{"type":"dahai","actor":2,"pai":"9m","tsumogiri":false}
{"type":"pon","actor":0,"target":2,"pai":"9m","consumed":["9m","9m"]}
{"type":"dahai","actor":0,"pai":"3s","tsumogiri":false}
{"type":"tsumo","actor":1,"pai":"7p"}
{"type":"dahai","actor":1,"pai":"3s","tsumogiri":false}
{"type":"tsumo","actor":2,"pai":"9p"}
{"type":"dahai","actor":2,"pai":"C","tsumogiri":false}
{"type":"tsumo","actor":0,"pai":"S"}
{"type":"dahai","actor":0,"pai":"S","tsumogiri":true}
{"type":"tsumo","actor":1,"pai":"4s"}
{"type":"dahai","actor":1,"pai":"4s","tsumogiri":true}
{"type":"tsumo","actor":2,"pai":"8p"}
{"type":"dahai","actor":2,"pai":"E","tsumogiri":false}
{"type":"tsumo","actor":0,"pai":"6p"}
{"type":"dahai","actor":0,"pai":"3p","tsumogiri":false}
{"type":"tsumo","actor":1,"pai":"7s"}
{"type":"dahai","actor":1,"pai":"7s","tsumogiri":true}
{"type":"tsumo","actor":2,"pai":"6s"}
{"type":"dahai","actor":2,"pai":"9p","tsumogiri":false}
{"type":"tsumo","actor":0,"pai":"7p"}
{"type":"dahai","actor":0,"pai":"7p","tsumogiri":true}
{"type":"tsumo","actor":1,"pai":"N"}
{"type":"nukidora","actor":1,"pai":"N"}
{"type":"tsumo","actor":1,"pai":"1p"}
{"type":"dahai","actor":1,"pai":"E","tsumogiri":false}
{"type":"tsumo","actor":2,"pai":"9m"}
{"type":"dahai","actor":2,"pai":"9m","tsumogiri":true}
{"type":"tsumo","actor":0,"pai":"7s"}
{"type":"dahai","actor":0,"pai":"7s","tsumogiri":true}
{"type":"tsumo","actor":1,"pai":"5s"}
{"type":"dahai","actor":1,"pai":"5s","tsumogiri":true}
{"type":"tsumo","actor":2,"pai":"F"}
{"type":"dahai","actor":2,"pai":"F","tsumogiri":true}
{"type":"tsumo","actor":1,"pai":"2s"}
{"type":"dahai","actor":1,"pai":"2s","tsumogiri":true}
{"type":"tsumo","actor":2,"pai":"8p"}
{"type":"dahai","actor":2,"pai":"9s","tsumogiri":false}
{"type":"tsumo","actor":1,"pai":"1s"}
{"type":"dahai","actor":1,"pai":"F","tsumogiri":false}
{"type":"pon","actor":0,"target":1,"pai":"F","consumed":["F","F"]}
{"type":"dahai","actor":0,"pai":"E","tsumogiri":false}
{"type":"tsumo","actor":1,"pai":"2p"}
{"type":"dahai","actor":1,"pai":"1s","tsumogiri":false}
{"type":"tsumo","actor":2,"pai":"1m"}
{"type":"dahai","actor":2,"pai":"1m","tsumogiri":true}
{"type":"tsumo","actor":0,"pai":"W"}
{"type":"dahai","actor":0,"pai":"W","tsumogiri":true}
{"type":"tsumo","actor":2,"pai":"4s"}
{"type":"dahai","actor":2,"pai":"1s","tsumogiri":false}
{"type":"tsumo","actor":0,"pai":"W"}
{"type":"dahai","actor":0,"pai":"W","tsumogiri":true}
{"type":"tsumo","actor":1,"pai":"2s"}
{"type":"dahai","actor":1,"pai":"2s","tsumogiri":true}
{"type":"hora","actor":2,"target":1,"deltas":[0,-5200,5200],"ura_markers":[]}
{"type":"end_kyoku"}
"#;

    fn events_from_jsonl(raw: &str) -> Vec<Event> {
        raw.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| json::from_str(line).expect("test event should deserialize"))
            .collect()
    }

    #[test]
    fn validator_accepts_real_tanyao_terminal_cleanup_ron() {
        let events = events_from_jsonl(TANYAO_RON_VALID_JSONL);
        validate_events(&events).expect("real tanyao ron edge should validate");
    }

    #[test]
    fn validator_rejects_ron_if_terminal_cleanup_turn_is_missing() {
        let mut events = events_from_jsonl(TANYAO_RON_VALID_JSONL);
        let terminal_cleanup = events
            .windows(4)
            .position(|window| {
                matches!(
                    window[0],
                    Event::Tsumo {
                        actor: 2,
                        pai,
                    } if pai == t!(4s)
                ) && matches!(
                    window[1],
                    Event::Dahai {
                        actor: 2,
                        pai,
                        tsumogiri: false,
                    } if pai == t!(1s)
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
                )
            })
            .expect("terminal cleanup window should exist");
        events.drain(terminal_cleanup..terminal_cleanup + 4);

        let err = validate_events(&events).expect_err("missing cleanup should make ron illegal");
        assert!(
            err.to_string().contains("can_ron_agari"),
            "unexpected validation error: {err:#}"
        );
    }

    #[test]
    fn validator_rejects_riichi_when_draw_discard_is_not_tenpai() {
        let events = vec![
            Event::StartGame {
                names: vec!["a".into(), "b".into(), "c".into()],
                seed: None,
            },
            Event::StartKyoku {
                bakaze: t!(E),
                dora_marker: t!(7s),
                kyoku: 1,
                honba: 0,
                kyotaku: 0,
                oya: 0,
                scores: vec![35000, 35000, 35000],
                tehais: vec![
                    vec![
                        t!(1p),
                        t!(2p),
                        t!(3p),
                        t!(4p),
                        t!(5p),
                        t!(6p),
                        t!(7p),
                        t!(8p),
                        t!(9p),
                        t!(E),
                        t!(S),
                        t!(W),
                        t!(P),
                    ],
                    vec![
                        t!(1p),
                        t!(2p),
                        t!(3p),
                        t!(4p),
                        t!(7p),
                        t!(1s),
                        t!(2s),
                        t!(3s),
                        t!(4s),
                        t!(S),
                        t!(P),
                        t!(F),
                        t!(F),
                    ],
                    vec![
                        t!(1m),
                        t!(9m),
                        t!(2p),
                        t!(3p),
                        t!(4p),
                        t!(8p),
                        t!(1s),
                        t!(3s),
                        t!(4s),
                        t!(5s),
                        t!(9s),
                        t!(E),
                        t!(C),
                    ],
                ],
            },
            Event::Tsumo {
                actor: 1,
                pai: t!(9p),
            },
            Event::Reach { actor: 1 },
        ];

        let err = validate_events(&events).expect_err("one-shanten riichi should be rejected");
        assert!(
            err.to_string().contains("can_riichi"),
            "unexpected validation error: {err:#}"
        );
    }

    #[test]
    fn validator_rejects_chi_in_sanma_log() {
        let events = vec![Event::Chi {
            actor: 1,
            target: 0,
            pai: t!(3p),
            consumed: [t!(1p), t!(2p)],
        }];

        let err = validate_events(&events).expect_err("sanma chi should be rejected");
        assert!(
            err.to_string().contains("sanma log contains chi"),
            "unexpected validation error: {err:#}"
        );
    }
}
