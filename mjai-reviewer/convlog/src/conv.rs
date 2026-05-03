use crate::Tile;
use crate::mjai::Event;
use crate::t;
use crate::tenhou::{ActionItem, EndStatus, Kyoku, Log, TenhouTile};
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::rc::Rc;

use ahash::{AHashMap, AHashSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("invalid naki string: {0:?}")]
    InvalidNaki(String),

    #[error("invalid tile string: {0:?}")]
    InvalidTile(String),

    #[error("sanma log contains four-player-only tile: {0}")]
    InvalidSanmaTile(Tile),

    #[error("sanma log contains chi event")]
    SanmaChi,

    #[error(
        "illegal sanma riichi: at kyoku {kyoku} honba {honba} for actor {actor}, \
        live wall has {tiles_left} tiles left"
    )]
    IllegalSanmaRiichi {
        kyoku: u8,
        honba: u8,
        actor: u8,
        tiles_left: u8,
    },

    #[error("insufficient dora indicators: at kyoku {kyoku} honba {honba}")]
    InsufficientDoraIndicators { kyoku: u8, honba: u8 },

    #[error(
        "insufficient take sequence size: \
        at kyoku {kyoku} honba {honba} for actor {actor}"
    )]
    InsufficientTakes { kyoku: u8, honba: u8, actor: u8 },

    #[error(
        "insufficient discard sequence size: \
        at kyoku {kyoku} honba {honba} for actor {actor}"
    )]
    InsufficientDiscards { kyoku: u8, honba: u8, actor: u8 },

    #[error("tsumogiri should not exist in discard table")]
    UnexpectedTsumogiri,

    #[error(
        "unexpected naki: \
        at kyoku {kyoku} honba {honba} for actor {actor}: \
        action {action:?}, expected tile {last_discard} \
        from {last_actor:?}"
    )]
    UnexpectedNaki {
        action: Event,
        last_discard: Tile,
        last_actor: Option<u8>,
        kyoku: u8,
        honba: u8,
        actor: u8,
    },
}

pub type Result<T> = std::result::Result<T, ConvertError>;

#[derive(Debug)]
struct BackTrack {
    use_the_first_branch: bool,
}

/// Transform a tenhou.net/6 format log into mjai format.
pub fn tenhou_to_mjai(log: &Log) -> Result<Vec<Event>> {
    let mut events = vec![Event::StartGame {
        kyoku_first: log.game_length as u8,
        aka_flag: log.has_aka,
        names: log.names.clone(),
    }];

    for kyoku in &log.kyokus {
        let kyoku_events = tenhou_kyoku_to_mjai_events(kyoku)?;
        events.extend(kyoku_events);
    }

    events.push(Event::EndGame);
    if log.num_players == 3 {
        validate_sanma_mjai(&events)?;
    }
    Ok(events)
}

fn tenhou_kyoku_to_mjai_events(kyoku: &Kyoku) -> Result<Vec<Event>> {
    let num_players = kyoku.action_tables.len();
    // First of all, transform all takes and discards to events.
    let (take_events, discard_events): (Vec<_>, Vec<_>) = (0..num_players)
        .map(|a| {
            parse_takes_and_discards_to_mjai(
                a as u8,
                num_players,
                &kyoku.action_tables[a as usize].takes,
                &kyoku.action_tables[a as usize].discards,
            )
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .unzip();

    // Then emit the events in order.
    // Tenhou sanma still uses the four-player round numbering grid: E1-E3
    // are 0,1,2 and S1 starts at 4. Seat 3 is skipped, but round display and
    // dealer identity must be derived with mod/div 4.
    let oya = kyoku.meta.kyoku_num % 4;
    let bakaze = match kyoku.meta.kyoku_num / 4 {
        0 => t!(E),
        1 => t!(S),
        2 => t!(W),
        _ => t!(N),
    };

    if num_players == 3 {
        return tenhou_sanma_kyoku_to_mjai_events(
            kyoku,
            &take_events,
            &discard_events,
            oya,
            bakaze,
        );
    }

    let attempt = |backtracks: &mut AHashMap<Tile, BackTrack>,
                   turn_step: usize|
     -> Result<Vec<Event>> {
        let mut events = vec![];

        let mut dora_feed = kyoku.dora_indicators.clone().into_iter();
        events.push(Event::StartKyoku {
            bakaze,
            kyoku: kyoku.meta.kyoku_num % 4 + 1,
            honba: kyoku.meta.honba,
            kyotaku: kyoku.meta.kyotaku,
            dora_marker: dora_feed
                .next()
                .ok_or(ConvertError::InsufficientDoraIndicators {
                    kyoku: kyoku.meta.kyoku_num,
                    honba: kyoku.meta.honba,
                })?,
            oya,
            scores: kyoku.scoreboard.clone(),
            tehais: kyoku
                .action_tables
                .iter()
                .map(|table| table.haipai.clone())
                .collect(),
        });

        let mut discard_sets: Vec<_> = (0..num_players)
            .map(|a| {
                let mut m = AHashMap::new();
                for discard in &discard_events[a] {
                    if let Event::Dahai { pai, .. } = *discard {
                        m.entry(pai).and_modify(|v| *v += 1).or_insert(1);
                    }
                }
                m
            })
            .collect();
        let mut take_idxs = vec![0; num_players];
        let mut discard_idxs = vec![0; num_players];
        let mut pending_nukidora = vec![0; num_players];
        let mut hand_counts: Vec<[u8; 38]> = kyoku
            .action_tables
            .iter()
            .map(|table| {
                let mut counts = [0; 38];
                for pai in &table.haipai {
                    counts[pai.as_usize()] += 1;
                }
                counts
            })
            .collect();

        let mut reach_flag: Option<usize> = None;
        let mut last_discard = t!(?);
        let mut last_actor: Option<u8> = None;
        let mut need_new_dora_at_discard = false;
        // This is for Kakan only because chankan is possible until an actual
        // tsumo.
        let mut need_new_dora_at_tsumo = false;

        let mut actor = oya as usize;

        loop {
            // Tenhou lists kita actions in the discard stream, including
            // norths extracted from the initial hand. In mjai replay order, the
            // draw/replacement draw is emitted before that nukidora. Queue the
            // f44 here and emit it after consuming the take.
            while matches!(
                discard_events[actor].get(discard_idxs[actor]),
                Some(Event::Nukidora { .. })
            ) && !is_final_ron_target_drawn_nukidora(
                kyoku,
                actor,
                discard_idxs[actor],
                &discard_events,
                &hand_counts,
            ) {
                pending_nukidora[actor] += 1;
                discard_idxs[actor] += 1;
            }

            // Start to process a take event.
            let Some(raw_take) = take_events[actor].get(take_idxs[actor]) else {
                if num_players == 3 {
                    if (0..num_players).all(|a| take_idxs[a] >= take_events[a].len()) {
                        end_kyoku(&mut events, kyoku);
                        break;
                    }
                    actor = next_actor_with_take(
                        num_players,
                        actor,
                        turn_step,
                        &take_idxs,
                        &take_events,
                    );
                    continue;
                }
                return Err(ConvertError::InsufficientTakes {
                    kyoku: kyoku.meta.kyoku_num,
                    honba: kyoku.meta.honba,
                    actor: actor as u8,
                });
            };
            let mut take = raw_take.clone();
            take_idxs[actor] += 1;

            if let Some((_, pai)) = take.naki_info() {
                if num_players == 3 && pai == last_discard {
                    if let Some(last_actor) = last_actor {
                        retarget_naki(&mut take, last_actor);
                    }
                }
            }

            if let Some((target, pai)) = take.naki_info() {
                if pai != last_discard
                    || last_actor.is_some_and(|a| a != target || a == actor as u8)
                {
                    return Err(ConvertError::UnexpectedNaki {
                        action: take,
                        last_discard,
                        last_actor,
                        kyoku: kyoku.meta.kyoku_num,
                        honba: kyoku.meta.honba,
                        actor: actor as u8,
                    });
                }
            }

            // If a reach event was emitted before, set it as accepted now.
            if let Some(actor) = reach_flag.take() {
                events.push(Event::ReachAccepted { actor: actor as u8 });
            }

            // If the take is daiminkan, immediately consume the next take event
            // from the same actor.
            match &take {
                Event::Daiminkan { .. } => {
                    // Not sure if this is really needed.
                    if need_new_dora_at_discard {
                        events.push(Event::Dora {
                            dora_marker: dora_feed.next().ok_or(
                                ConvertError::InsufficientDoraIndicators {
                                    kyoku: kyoku.meta.kyoku_num,
                                    honba: kyoku.meta.honba,
                                },
                            )?,
                        });
                    }

                    events.push(take);
                    need_new_dora_at_discard = true;
                    continue;
                }

                // This is for Kakan only because chankan is possible until an
                // actual tsumo.
                Event::Tsumo { .. } if need_new_dora_at_tsumo => {
                    events.push(Event::Dora {
                        dora_marker: dora_feed.next().ok_or(
                            ConvertError::InsufficientDoraIndicators {
                                kyoku: kyoku.meta.kyoku_num,
                                honba: kyoku.meta.honba,
                            },
                        )?,
                    });
                    need_new_dora_at_tsumo = false;
                }

                _ => (),
            };

            // Emit the take event.
            events.push(take.clone());
            apply_take_to_hand(&mut hand_counts[actor], &take);

            if pending_nukidora[actor] > 0 && matches!(take, Event::Tsumo { .. }) {
                hand_counts[actor][t!(N).as_usize()] =
                    hand_counts[actor][t!(N).as_usize()].saturating_sub(1);
                events.push(Event::Nukidora {
                    actor: actor as u8,
                    pai: t!(N),
                });
                pending_nukidora[actor] -= 1;
                if (0..num_players).all(|a| take_idxs[a] >= take_events[a].len()) {
                    end_kyoku(&mut events, kyoku);
                    break;
                }
                continue;
            }

            if matches!(take, Event::Tsumo { pai, .. } if pai == t!(N))
                && matches!(
                    discard_events[actor].get(discard_idxs[actor]),
                    Some(Event::Nukidora { .. })
                )
            {
                hand_counts[actor][t!(N).as_usize()] -= 1;
                events.push(Event::Nukidora {
                    actor: actor as u8,
                    pai: t!(N),
                });
                discard_idxs[actor] += 1;
                if (0..num_players).all(|a| take_idxs[a] >= take_events[a].len()) {
                    end_kyoku(&mut events, kyoku);
                    break;
                }
                if is_ron_target(kyoku, actor as u8)
                    && discard_idxs[actor] + 1 == discard_events[actor].len()
                {
                    actor = next_actor_with_take(
                        num_players,
                        actor,
                        turn_step,
                        &take_idxs,
                        &take_events,
                    );
                }
                continue;
            }

            // Check if the kyoku ends here, can be ryukyoku (九種九牌) or tsumo.
            // Here it simply checks if there is no more discard for current actor.
            if discard_idxs[actor] >= discard_events[actor].len() {
                end_kyoku(&mut events, kyoku);
                break;
            }

            // Start to process a discard event.
            let discard = discard_events[actor]
                .get(discard_idxs[actor])
                .ok_or(ConvertError::InsufficientDiscards {
                    kyoku: kyoku.meta.kyoku_num,
                    honba: kyoku.meta.honba,
                    actor: actor as u8,
                })?
                .clone();
            discard_idxs[actor] += 1;

            // Record the pai to check if someone naki it.
            if let Event::Dahai { pai, .. } = discard {
                last_discard = pai;
                discard_sets[actor].entry(pai).and_modify(|v| *v -= 1);
            }

            // Process previous minkan.
            if need_new_dora_at_discard {
                match discard {
                    Event::Dahai { .. } | Event::Ankan { .. } => {
                        events.push(Event::Dora {
                            dora_marker: dora_feed.next().ok_or(
                                ConvertError::InsufficientDoraIndicators {
                                    kyoku: kyoku.meta.kyoku_num,
                                    honba: kyoku.meta.honba,
                                },
                            )?,
                        });
                        need_new_dora_at_discard = false;
                    }

                    Event::Kakan { .. } => {
                        need_new_dora_at_tsumo = true;
                    }
                    _ => (),
                };
            }

            // Emit the discard event.
            events.push(discard.clone());
            apply_discard_to_hand(&mut hand_counts[actor], &discard);

            // Process reach declare.
            //
            // A reach declare consists of two events (reach
            // + dahai).
            if let Event::Reach { .. } = discard {
                reach_flag = Some(actor);

                let dahai = discard_events[actor]
                    .get(discard_idxs[actor])
                    .ok_or(ConvertError::InsufficientDiscards {
                        kyoku: kyoku.meta.kyoku_num,
                        honba: kyoku.meta.honba,
                        actor: actor as u8,
                    })?
                    .clone();
                discard_idxs[actor] += 1;
                if let Event::Dahai { pai, .. } = dahai {
                    last_discard = pai;
                    discard_sets[actor].entry(pai).and_modify(|v| *v -= 1);
                }
                apply_discard_to_hand(&mut hand_counts[actor], &dahai);
                events.push(dahai);

                if is_final_ron_discard(
                    kyoku,
                    actor as u8,
                    &discard_idxs,
                    &discard_events,
                    &take_idxs,
                    &take_events,
                ) {
                    end_kyoku(&mut events, kyoku);
                    break;
                }
            }

            if is_final_ron_discard(
                kyoku,
                actor as u8,
                &discard_idxs,
                &discard_events,
                &take_idxs,
                &take_events,
            ) {
                end_kyoku(&mut events, kyoku);
                break;
            }

            // Check if the kyoku ends here, can be ryukyoku or ron.
            //
            // Here it simply checks if there is no more take for every single
            // actor.
            if (0..num_players).all(|a| take_idxs[a] >= take_events[a].len()) {
                if can_end_after_exhausted_takes(
                    kyoku,
                    actor as u8,
                    &discard_idxs,
                    &discard_events,
                    &take_idxs,
                    &take_events,
                ) {
                    end_kyoku(&mut events, kyoku);
                    break;
                }
                return Err(ConvertError::UnexpectedNaki {
                    action: discard,
                    last_discard,
                    last_actor,
                    kyoku: kyoku.meta.kyoku_num,
                    honba: kyoku.meta.honba,
                    actor: actor as u8,
                });
            }

            // Check if the last discard was ankan or kakan.
            //
            // For kan, it will immediately consume the next take event from the
            // same actor.
            match discard {
                Event::Ankan { .. } => {
                    // ankan triggers a dora event immediately.
                    events.push(Event::Dora {
                        dora_marker: dora_feed.next().ok_or(
                            ConvertError::InsufficientDoraIndicators {
                                kyoku: kyoku.meta.kyoku_num,
                                honba: kyoku.meta.honba,
                            },
                        )?,
                    });
                    continue;
                }
                Event::Kakan { .. } => {
                    need_new_dora_at_discard = true;
                    continue;
                }
                Event::Nukidora { .. } => {
                    continue;
                }
                _ => (),
            }

            // Decide who is the next actor.
            //
            // For most of the time, if someone takes naki of the previous discard,
            // then it will be him, otherwise it will be the shimocha.
            //
            // There are some edge cases when there are multiple candidates for the
            // next actor, which will be handled by the second pass of the filter.
            last_actor = Some(actor as u8);
            let naki_actor = (0..num_players)
                .filter(|&a| a != actor)
                // First pass, filter the naki that takes the specific tile from the
                // specific target.
                .filter_map(|a| {
                    if let Some(take) = take_events[a].get(take_idxs[a]) {
                        if let Some((target, pai)) = take.naki_info() {
                            if (num_players == 3 || target == actor as u8) && pai == last_discard {
                                return Some((a, take.naki_to_ord()));
                            }
                        }
                    }

                    None
                })
                // Second pass, compare the nakis and filter out the final
                // candidate.
                //
                // If a Chi and a Pon that calls the same tile from the same actor
                // can take place at the same time, then Pon must be the first to
                // take place, because if the Chi is the first instead, then the Pon
                // will be impossible to take as he will have no chance to Pon from
                // the same actor without Tsumo first.
                //
                // There is one exception to make the Chi legal though - the actor
                // takes another naki (Pon) before him, which is rare to be seen and
                // it seems not possible to properly describe it on tenhou.net/6.
                .max_by_key(|&(_, naki_ord)| naki_ord)
                .map(|(a, _)| a)
                // Backtracking, mitigate the real-naki-of-two-identical-discard
                // problem. If you are wondering, check `confusing_nakis` in
                // testdata and load them into tenhou.net/6 to see what the problem
                // is.
                //
                // Basically, the condition of such problem to occur is when actor A
                // discard the exact same pai at the next step, without giving actor
                // B any chance to tsumo, while actor B actually pon'd this pai. In
                // the end, we are not sure which one of the two identical dahais
                // actor A make is corresponding to actor B's pon.
                //
                // I really can't think of a better way to solve this.
                .and_then(|a| {
                    if discard_idxs[actor] >= discard_events[actor].len() {
                        // There is no more discard for this actor, so no chance for
                        // the problem to exist.
                        return Some(a);
                    }

                    let has_same_dahai_in_future = discard_sets[actor]
                        .get(&last_discard)
                        .is_some_and(|&v| v > 0);
                    if !has_same_dahai_in_future {
                        // no candidate
                        return Some(a);
                    }

                    match backtracks.entry(last_discard) {
                        Entry::Vacant(v) => {
                            // Try taking the first dahai as the real naki.
                            v.insert(BackTrack {
                                use_the_first_branch: true,
                            });
                            Some(a)
                        }
                        Entry::Occupied(mut o) => {
                            // This is where the backtrack happens.
                            let bc = o.get_mut();
                            if bc.use_the_first_branch {
                                // When this branch is reached, it is likely the
                                // first branch has failed, that is, the real naki
                                // doesn't seem to be the first discard, so we will
                                // try the second discard.
                                bc.use_the_first_branch = false;
                            } else {
                                // Both branches are wrong, backtrack further to the
                                // previous point of divergence.
                                //
                                // None is still returned here to trigger an error
                                // at the end of the outer function so that the
                                // backtrack can continue.
                                o.remove_entry();
                            }
                            None
                        }
                    }
                });

            actor = naki_actor.unwrap_or_else(|| {
                let mut next = next_turn_actor(num_players, actor, turn_step);
                for _ in 0..num_players {
                    // Skip actors with no remaining takes (exhausted due to uneven
                    // pon distribution in sanma). is_some_and ensures we only pick
                    // an actor who actually has a take available and isn't about to naki.
                    if take_events[next]
                        .get(take_idxs[next])
                        .is_some_and(|event| event.naki_info().is_none())
                    {
                        return next;
                    }
                    next = next_turn_actor(num_players, next, turn_step);
                }
                next_turn_actor(num_players, actor, turn_step)
            });
        }

        Ok(events)
    };

    let mut first_error = None;
    let mut backtracks = AHashMap::new();
    loop {
        match attempt(&mut backtracks, 1) {
            Ok(events) => return Ok(events),
            Err(err) => {
                first_error = first_error.or(Some(err));
                if backtracks.is_empty() {
                    break;
                }
            }
        }
    }

    Err(first_error.unwrap())
}

#[derive(Clone)]
struct SanmaReplayState<'a> {
    kyoku: &'a Kyoku,
    take_events: &'a [Vec<Event>],
    discard_events: &'a [Vec<Event>],
    events: Vec<Event>,
    dora_idx: usize,
    take_idxs: Vec<usize>,
    discard_idxs: Vec<usize>,
    hand_counts: Vec<[u8; 38]>,
    meld_counts: Vec<u8>,
    discarded_tiles: Vec<[bool; 34]>,
    actor: usize,
    live_wall_left: u8,
    reach_flag: Option<usize>,
    last_discard: Tile,
    last_actor: Option<u8>,
    need_new_dora_at_discard: bool,
    need_new_dora_at_tsumo: bool,
    riichi_accepted: [bool; 3],
    temporary_furiten: [bool; 3],
    riichi_furiten: [bool; 3],
    failed_states: Rc<RefCell<AHashSet<SanmaReplayKey>>>,
    steps: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SanmaReplayKey {
    actor: usize,
    take_idxs: [usize; 3],
    discard_idxs: [usize; 3],
    live_wall_left: u8,
    reach_flag: Option<usize>,
    last_discard: u8,
    last_actor: Option<u8>,
    dora_idx: usize,
    need_new_dora_at_discard: bool,
    need_new_dora_at_tsumo: bool,
    riichi_accepted: [bool; 3],
    temporary_furiten: [bool; 3],
    riichi_furiten: [bool; 3],
    discarded_tiles: [[bool; 34]; 3],
}

#[derive(Clone, Copy)]
struct SanmaCallCandidate {
    actor: usize,
    take_idx: usize,
    discard_idx: usize,
    ord: i8,
    original_target: u8,
    direct_target: bool,
    skipped_self_tsumogiri_turns: usize,
}

#[derive(Clone, Copy)]
struct SanmaActorCandidate {
    actor: usize,
    take_idx: usize,
    discard_idx: usize,
    turn_order: usize,
    skipped_self_tsumogiri_turns: usize,
}

fn tenhou_sanma_kyoku_to_mjai_events(
    kyoku: &Kyoku,
    take_events: &[Vec<Event>],
    discard_events: &[Vec<Event>],
    oya: u8,
    bakaze: Tile,
) -> Result<Vec<Event>> {
    let dora_marker =
        *kyoku
            .dora_indicators
            .first()
            .ok_or(ConvertError::InsufficientDoraIndicators {
                kyoku: kyoku.meta.kyoku_num,
                honba: kyoku.meta.honba,
            })?;
    let events = vec![Event::StartKyoku {
        bakaze,
        kyoku: kyoku.meta.kyoku_num % 4 + 1,
        honba: kyoku.meta.honba,
        kyotaku: kyoku.meta.kyotaku,
        dora_marker,
        oya,
        scores: kyoku.scoreboard.clone(),
        tehais: kyoku
            .action_tables
            .iter()
            .map(|table| table.haipai.clone())
            .collect(),
    }];
    let hand_counts: Vec<[u8; 38]> = kyoku
        .action_tables
        .iter()
        .map(|table| {
            let mut counts = [0; 38];
            for pai in &table.haipai {
                counts[pai.as_usize()] += 1;
            }
            counts
        })
        .collect();

    let state = SanmaReplayState {
        kyoku,
        take_events,
        discard_events,
        events,
        dora_idx: 1,
        take_idxs: vec![0; 3],
        discard_idxs: vec![0; 3],
        hand_counts,
        meld_counts: vec![0; 3],
        discarded_tiles: vec![[false; 34]; 3],
        actor: oya as usize,
        live_wall_left: 55,
        reach_flag: None,
        last_discard: t!(?),
        last_actor: None,
        need_new_dora_at_discard: false,
        need_new_dora_at_tsumo: false,
        riichi_accepted: [false; 3],
        temporary_furiten: [false; 3],
        riichi_furiten: [false; 3],
        failed_states: Rc::new(RefCell::new(AHashSet::new())),
        steps: 0,
    };

    run_sanma_replay(state)
}

fn run_sanma_replay(state: SanmaReplayState<'_>) -> Result<Vec<Event>> {
    let key = state.replay_key();
    let seen = state.failed_states.borrow().contains(&key);
    if seen {
        return Err(unexpected_state_error(&state, Event::None));
    }

    let failed_states = Rc::clone(&state.failed_states);
    let result = run_sanma_replay_inner(state);
    if result.is_err() {
        failed_states.borrow_mut().insert(key);
    }
    result
}

fn run_sanma_replay_inner(mut state: SanmaReplayState<'_>) -> Result<Vec<Event>> {
    const MAX_REPLAY_STEPS: usize = 4096;
    state.steps += 1;
    if state.steps > MAX_REPLAY_STEPS {
        return Err(unexpected_state_error(&state, Event::None));
    }

    if state.take_idxs[state.actor] >= state.take_events[state.actor].len() {
        if state.all_takes_consumed()
            && can_end_after_exhausted_takes(
                state.kyoku,
                state.last_actor.unwrap_or(state.actor as u8),
                &state.discard_idxs,
                state.discard_events,
                &state.take_idxs,
                state.take_events,
            )
        {
            end_kyoku(&mut state.events, state.kyoku);
            return Ok(state.events);
        }

        if let Some(candidate) = state.normal_actor_candidates().into_iter().next() {
            state.actor = candidate.actor;
            state.take_idxs[candidate.actor] = candidate.take_idx;
            state.discard_idxs[candidate.actor] = candidate.discard_idx;
            return run_sanma_replay(state);
        }

        return Err(ConvertError::InsufficientTakes {
            kyoku: state.kyoku.meta.kyoku_num,
            honba: state.kyoku.meta.honba,
            actor: state.actor as u8,
        });
    }

    if let Some(actor) = state.reach_flag.take() {
        state
            .events
            .push(Event::ReachAccepted { actor: actor as u8 });
        state.riichi_accepted[actor] = true;
    }

    let mut take = state.take_events[state.actor][state.take_idxs[state.actor]].clone();
    state.take_idxs[state.actor] += 1;
    if let Some((_, pai)) = take.naki_info() {
        if pai == state.last_discard {
            if let Some(last_actor) = state.last_actor {
                retarget_naki(&mut take, last_actor);
            }
        }
    }
    if let Some((target, pai)) = take.naki_info() {
        if pai != state.last_discard
            || state
                .last_actor
                .is_some_and(|a| a != target || a == state.actor as u8)
        {
            return Err(ConvertError::UnexpectedNaki {
                action: take,
                last_discard: state.last_discard,
                last_actor: state.last_actor,
                kyoku: state.kyoku.meta.kyoku_num,
                honba: state.kyoku.meta.honba,
                actor: state.actor as u8,
            });
        }
    }

    match &take {
        Event::Tsumo { .. } => {
            if state.live_wall_left == 0 {
                return Err(unexpected_state_error(&state, take.clone()));
            }
            state.live_wall_left -= 1;
        }
        _ => (),
    }

    match &take {
        Event::Daiminkan { .. } => {
            if state.need_new_dora_at_discard {
                state.push_dora()?;
            }
            state.apply_take(&take)?;
            state.events.push(take);
            state.need_new_dora_at_discard = true;
            return run_sanma_replay(state);
        }
        Event::Tsumo { .. } if state.need_new_dora_at_tsumo => {
            state.push_dora()?;
            state.need_new_dora_at_tsumo = false;
        }
        _ => (),
    }

    state.apply_take(&take)?;
    state.events.push(take);

    if matches!(
        state.discard_events[state.actor].get(state.discard_idxs[state.actor]),
        Some(Event::Nukidora { .. })
    ) {
        let nukidora = state.discard_events[state.actor][state.discard_idxs[state.actor]].clone();
        state.discard_idxs[state.actor] += 1;
        let Event::Nukidora { pai: ron_tile, .. } = nukidora else {
            unreachable!("matched nukidora above")
        };
        state.apply_discard(&nukidora)?;
        state.events.push(nukidora);
        if state.is_ron_moment(state.actor) {
            if state.ron_moment_blocked_by_furiten(state.actor, ron_tile, false) {
                return Err(unexpected_state_error(&state, Event::None));
            }
            end_kyoku(&mut state.events, state.kyoku);
            return Ok(state.events);
        }
        return run_sanma_replay(state);
    }

    if state.discard_idxs[state.actor] >= state.discard_events[state.actor].len() {
        if state.can_end_without_discard() {
            end_kyoku(&mut state.events, state.kyoku);
            return Ok(state.events);
        }
        return Err(ConvertError::InsufficientDiscards {
            kyoku: state.kyoku.meta.kyoku_num,
            honba: state.kyoku.meta.honba,
            actor: state.actor as u8,
        });
    }

    let discard = state.discard_events[state.actor][state.discard_idxs[state.actor]].clone();
    state.discard_idxs[state.actor] += 1;
    state.process_discard(discard)
}

impl SanmaReplayState<'_> {
    fn replay_key(&self) -> SanmaReplayKey {
        SanmaReplayKey {
            actor: self.actor,
            take_idxs: [self.take_idxs[0], self.take_idxs[1], self.take_idxs[2]],
            discard_idxs: [
                self.discard_idxs[0],
                self.discard_idxs[1],
                self.discard_idxs[2],
            ],
            live_wall_left: self.live_wall_left,
            reach_flag: self.reach_flag,
            last_discard: self.last_discard.as_u8(),
            last_actor: self.last_actor,
            dora_idx: self.dora_idx,
            need_new_dora_at_discard: self.need_new_dora_at_discard,
            need_new_dora_at_tsumo: self.need_new_dora_at_tsumo,
            riichi_accepted: self.riichi_accepted,
            temporary_furiten: self.temporary_furiten,
            riichi_furiten: self.riichi_furiten,
            discarded_tiles: [
                self.discarded_tiles[0],
                self.discarded_tiles[1],
                self.discarded_tiles[2],
            ],
        }
    }

    fn process_discard(mut self, discard: Event) -> Result<Vec<Event>> {
        if self.need_new_dora_at_discard {
            match discard {
                Event::Dahai { .. } | Event::Ankan { .. } => {
                    self.push_dora()?;
                    self.need_new_dora_at_discard = false;
                }
                Event::Kakan { .. } => {
                    self.need_new_dora_at_tsumo = true;
                }
                _ => (),
            }
        }

        if let Event::Reach { .. } = discard {
            if self.live_wall_left < 3 {
                return Err(ConvertError::IllegalSanmaRiichi {
                    kyoku: self.kyoku.meta.kyoku_num,
                    honba: self.kyoku.meta.honba,
                    actor: self.actor as u8,
                    tiles_left: self.live_wall_left,
                });
            }
            self.events.push(discard);
            self.reach_flag = Some(self.actor);
            let dahai = self.discard_events[self.actor]
                .get(self.discard_idxs[self.actor])
                .ok_or(ConvertError::InsufficientDiscards {
                    kyoku: self.kyoku.meta.kyoku_num,
                    honba: self.kyoku.meta.honba,
                    actor: self.actor as u8,
                })?
                .clone();
            self.discard_idxs[self.actor] += 1;
            self.apply_discard(&dahai)?;
            if !is_tenpai_hand_shape(&self.hand_counts[self.actor], self.meld_counts[self.actor]) {
                return Err(unexpected_state_error(&self, dahai.clone()));
            }
            if let Event::Dahai { pai, .. } = dahai {
                self.last_discard = pai;
                self.last_actor = Some(self.actor as u8);
            }
            self.events.push(dahai);
            return self.after_discard();
        }

        self.apply_discard(&discard)?;
        match discard {
            Event::Dahai { pai, .. } => {
                self.last_discard = pai;
                self.last_actor = Some(self.actor as u8);
                self.events.push(discard);
                self.after_discard()
            }
            Event::Ankan { .. } => {
                self.events.push(discard);
                self.push_dora()?;
                run_sanma_replay(self)
            }
            Event::Kakan { pai, .. } => {
                self.events.push(discard);
                if self.is_ron_moment(self.actor) {
                    if self.ron_moment_blocked_by_furiten(self.actor, pai, false) {
                        return Err(unexpected_state_error(&self, Event::None));
                    }
                    end_kyoku(&mut self.events, self.kyoku);
                    return Ok(self.events);
                }
                self.need_new_dora_at_discard = true;
                run_sanma_replay(self)
            }
            Event::Nukidora { pai, .. } => {
                self.events.push(discard);
                if self.is_ron_moment(self.actor) {
                    if self.ron_moment_blocked_by_furiten(self.actor, pai, false) {
                        return Err(unexpected_state_error(&self, Event::None));
                    }
                    end_kyoku(&mut self.events, self.kyoku);
                    return Ok(self.events);
                }
                run_sanma_replay(self)
            }
            _ => Err(unexpected_state_error(&self, discard)),
        }
    }

    fn after_discard(mut self) -> Result<Vec<Event>> {
        if self.is_ron_moment(self.actor) {
            if self.ron_moment_blocked_by_furiten(self.actor, self.last_discard, true) {
                return Err(unexpected_state_error(&self, Event::None));
            }
            end_kyoku(&mut self.events, self.kyoku);
            return Ok(self.events);
        }

        self.mark_passed_discard_ron_opportunities();

        if self.all_takes_consumed() {
            if matches!(self.kyoku.end_status, EndStatus::Hora { .. }) {
                return Err(unexpected_state_error(&self, Event::None));
            }
            if can_end_after_exhausted_takes(
                self.kyoku,
                self.actor as u8,
                &self.discard_idxs,
                self.discard_events,
                &self.take_idxs,
                self.take_events,
            ) {
                let mut state = self;
                end_kyoku(&mut state.events, state.kyoku);
                return Ok(state.events);
            }
            return Err(unexpected_state_error(&self, Event::None));
        }

        let call_candidates = self.call_candidates();
        let (retarget_now, retarget_deferred): (Vec<_>, Vec<_>) =
            call_candidates.iter().copied().partition(|candidate| {
                candidate.direct_target
                    || !self.has_callable_future_discard_from(
                        candidate.original_target,
                        self.last_discard,
                    )
            });
        let may_skip_call = call_candidates.is_empty()
            || call_candidates.iter().any(|candidate| {
                candidate.direct_target && self.has_future_discard(self.last_discard)
            })
            || !retarget_deferred.is_empty();
        let mut first_error = None;

        for candidate in retarget_now {
            let mut branch = self.clone();
            branch.actor = candidate.actor;
            branch.take_idxs[candidate.actor] = candidate.take_idx;
            branch.discard_idxs[candidate.actor] = candidate.discard_idx;
            match run_sanma_replay(branch) {
                Ok(events) => return Ok(events),
                Err(err) => first_error = first_error.or(Some(err)),
            }
        }

        if may_skip_call {
            for candidate in self.normal_actor_candidates() {
                let mut branch = self.clone();
                branch.actor = candidate.actor;
                branch.take_idxs[candidate.actor] = candidate.take_idx;
                branch.discard_idxs[candidate.actor] = candidate.discard_idx;
                match run_sanma_replay(branch) {
                    Ok(events) => return Ok(events),
                    Err(err) => first_error = first_error.or(Some(err)),
                }
            }
        }

        for candidate in retarget_deferred {
            let mut branch = self.clone();
            branch.actor = candidate.actor;
            branch.take_idxs[candidate.actor] = candidate.take_idx;
            branch.discard_idxs[candidate.actor] = candidate.discard_idx;
            match run_sanma_replay(branch) {
                Ok(events) => return Ok(events),
                Err(err) => first_error = first_error.or(Some(err)),
            }
        }

        Err(first_error.unwrap_or_else(|| unexpected_state_error(&self, Event::None)))
    }

    fn call_candidates(&self) -> Vec<SanmaCallCandidate> {
        if self.live_wall_left == 0 {
            return Vec::new();
        }

        let mut candidates: Vec<_> = (0..3)
            .filter(|&actor| actor != self.actor)
            .filter_map(|actor| self.call_candidate_for_actor(actor))
            .collect();
        candidates.sort_by_key(|candidate| {
            (
                std::cmp::Reverse(candidate.ord),
                candidate.skipped_self_tsumogiri_turns,
            )
        });
        candidates
    }

    fn call_candidate_for_actor(&self, actor: usize) -> Option<SanmaCallCandidate> {
        let mut take_idx = self.take_idxs[actor];
        let mut discard_idx = self.discard_idxs[actor];
        let mut skipped_self_tsumogiri_turns = 0;
        loop {
            if let Some((ord, original_target, direct_target)) =
                self.matching_naki_ord(actor, take_idx)
            {
                return Some(SanmaCallCandidate {
                    actor,
                    take_idx,
                    discard_idx,
                    ord,
                    original_target,
                    direct_target,
                    skipped_self_tsumogiri_turns,
                });
            }

            if !self.is_skippable_self_tsumogiri_turn(actor, take_idx, discard_idx) {
                break;
            }

            skipped_self_tsumogiri_turns += 1;
            take_idx += 1;
            discard_idx += 1;
        }

        None
    }

    fn matching_naki_ord(&self, actor: usize, take_idx: usize) -> Option<(i8, u8, bool)> {
        let mut take = self.take_events[actor].get(take_idx)?.clone();
        let (target, pai) = take.naki_info()?;
        if pai != self.last_discard {
            return None;
        }
        let direct_target = self
            .last_actor
            .is_some_and(|last_actor| target == last_actor);
        retarget_naki(&mut take, self.actor as u8);
        take.naki_info()
            .is_some_and(|(target, _)| target == self.actor as u8)
            .then_some((take.naki_to_ord(), target, direct_target))
    }

    fn is_skippable_self_tsumogiri_turn(
        &self,
        actor: usize,
        take_idx: usize,
        discard_idx: usize,
    ) -> bool {
        // Some Tenhou sanma JSON logs leave a plain self draw/tsumogiri pair in
        // front of a later naki that actually claims the current discard. The
        // no-call branch would play that pair, but the call branch must consume
        // the stream in order while not emitting the unplayed self-turn.
        matches!(
            (
                self.take_events[actor].get(take_idx),
                self.discard_events[actor].get(discard_idx),
            ),
            (
                Some(Event::Tsumo { pai: tsumo, .. }),
                Some(Event::Dahai {
                    pai: dahai,
                    tsumogiri: true,
                    ..
                }),
            ) if tsumo == dahai
        )
    }

    fn normal_actor_candidates(&self) -> Vec<SanmaActorCandidate> {
        let mut candidates = Vec::new();
        let mut next = (self.actor + 1) % 3;
        for turn_order in 0..3 {
            let take_idx = self.take_idxs[next];
            let discard_idx = self.discard_idxs[next];
            if self.take_events[next]
                .get(take_idx)
                .is_some_and(|event| event.naki_info().is_none())
            {
                candidates.push(SanmaActorCandidate {
                    actor: next,
                    take_idx,
                    discard_idx,
                    turn_order,
                    skipped_self_tsumogiri_turns: 0,
                });
            }
            next = (next + 1) % 3;
        }
        candidates.sort_by_key(|candidate| {
            (
                candidate.turn_order,
                self.normal_candidate_causes_riichi_missed_ron(candidate),
                candidate.skipped_self_tsumogiri_turns,
            )
        });
        candidates
    }

    fn normal_candidate_causes_riichi_missed_ron(&self, candidate: &SanmaActorCandidate) -> bool {
        let Some(Event::Dahai { pai, .. }) =
            self.discard_events[candidate.actor].get(candidate.discard_idx)
        else {
            return false;
        };

        match &self.kyoku.end_status {
            EndStatus::Hora { details } => details.iter().any(|detail| {
                let who = detail.who as usize;
                who != candidate.actor
                    && self.riichi_accepted[who]
                    && is_complete_hand_shape_with_win_tile(
                        &self.hand_counts[who],
                        *pai,
                        self.meld_counts[who],
                    )
            }),
            EndStatus::Ryukyoku { .. } => false,
        }
    }

    fn has_future_discard(&self, pai: Tile) -> bool {
        self.discard_events
            .iter()
            .enumerate()
            .any(|(actor, discards)| {
                discards
                    .iter()
                    .skip(self.discard_idxs[actor])
                    .any(|event| matches!(event, Event::Dahai { pai: p, .. } if *p == pai))
            })
    }

    fn has_callable_future_discard_from(&self, actor: u8, pai: Tile) -> bool {
        let actor = actor as usize;
        if actor >= self.discard_events.len() {
            return false;
        }

        let mut take_idx = self.take_idxs[actor];
        let mut discard_idx = self.discard_idxs[actor];
        let mut future_tsumos = 0;

        while take_idx < self.take_events[actor].len() {
            if matches!(self.take_events[actor][take_idx], Event::Tsumo { .. }) {
                future_tsumos += 1;
            }
            take_idx += 1;

            loop {
                let Some(discard) = self.discard_events[actor].get(discard_idx) else {
                    return false;
                };
                discard_idx += 1;

                match discard {
                    Event::Dahai { pai: p, .. } => {
                        if *p == pai {
                            return future_tsumos < self.live_wall_left;
                        }
                        break;
                    }
                    Event::Reach { .. } => continue,
                    Event::Nukidora { .. } | Event::Ankan { .. } | Event::Kakan { .. } => break,
                    _ => return false,
                }
            }
        }

        false
    }

    fn mark_passed_discard_ron_opportunities(&mut self) {
        for actor in 0..3 {
            if actor == self.actor {
                continue;
            }
            if self.riichi_furiten[actor] || self.temporary_furiten[actor] {
                continue;
            }
            if is_complete_hand_shape_with_win_tile(
                &self.hand_counts[actor],
                self.last_discard,
                self.meld_counts[actor],
            ) {
                if self.riichi_accepted[actor] {
                    self.riichi_furiten[actor] = true;
                } else {
                    self.temporary_furiten[actor] = true;
                }
            }
        }
    }

    fn ron_moment_blocked_by_furiten(
        &self,
        target_actor: usize,
        win_tile: Tile,
        block_temporary_furiten: bool,
    ) -> bool {
        match &self.kyoku.end_status {
            EndStatus::Hora { details } => details
                .iter()
                .filter(|detail| detail.target == target_actor as u8 && detail.who != detail.target)
                .any(|detail| {
                    let who = detail.who as usize;
                    !is_complete_hand_shape_with_win_tile(
                        &self.hand_counts[who],
                        win_tile,
                        self.meld_counts[who],
                    ) || !self.result_yaku_timing_compatible(detail, block_temporary_furiten)
                        || !self.result_yaku_shape_compatible(detail, win_tile)
                        || self.is_furiten_by_own_discards(who)
                        || self.riichi_furiten[who]
                        || block_temporary_furiten && self.temporary_furiten[who]
                }),
            EndStatus::Ryukyoku { .. } => false,
        }
    }

    fn result_yaku_timing_compatible(
        &self,
        detail: &crate::tenhou::HoraDetail,
        normal_discard_ron: bool,
    ) -> bool {
        if detail.yaku.iter().any(|yaku| yaku.contains("立直"))
            && !self.riichi_accepted[detail.who as usize]
        {
            return false;
        }

        if detail.yaku.iter().any(|yaku| yaku.contains("河底")) && self.live_wall_left != 0 {
            return false;
        }

        if detail.yaku.iter().any(|yaku| yaku.contains("槍槓")) && normal_discard_ron {
            return false;
        }

        true
    }

    fn result_yaku_shape_compatible(
        &self,
        detail: &crate::tenhou::HoraDetail,
        win_tile: Tile,
    ) -> bool {
        if detail.yaku.iter().any(|yaku| yaku.contains("断幺九"))
            && !is_tanyao_shape_with_win_tile(&self.hand_counts[detail.who as usize], win_tile)
        {
            return false;
        }

        true
    }

    fn is_furiten_by_own_discards(&self, actor: usize) -> bool {
        self.discarded_tiles[actor]
            .iter()
            .enumerate()
            .any(|(tile_id, &discarded)| {
                discarded
                    && is_complete_hand_shape_with_win_tile(
                        &self.hand_counts[actor],
                        Tile::try_from(tile_id).expect("valid tile id"),
                        self.meld_counts[actor],
                    )
            })
    }

    fn all_takes_consumed(&self) -> bool {
        (0..3).all(|actor| self.take_idxs[actor] >= self.take_events[actor].len())
    }

    fn can_end_without_discard(&self) -> bool {
        match &self.kyoku.end_status {
            EndStatus::Hora { details } => details
                .iter()
                .any(|detail| detail.who == self.actor as u8 && detail.target == detail.who),
            EndStatus::Ryukyoku { .. } => true,
        }
    }

    fn is_ron_moment(&self, target_actor: usize) -> bool {
        if self.discard_idxs[target_actor] < self.discard_events[target_actor].len() {
            return false;
        }
        match &self.kyoku.end_status {
            EndStatus::Hora { details } => {
                let mut has_ron = false;
                for detail in details
                    .iter()
                    .filter(|d| d.target == target_actor as u8 && d.who != d.target)
                {
                    has_ron = true;
                    let who = detail.who as usize;
                    if self.take_idxs[who] < self.take_events[who].len()
                        && !self.has_only_trailing_phantom_winner_turns(who)
                    {
                        return false;
                    }
                }
                has_ron
            }
            EndStatus::Ryukyoku { .. } => false,
        }
    }

    fn has_only_trailing_phantom_winner_turns(&self, actor: usize) -> bool {
        if !is_ron_winner(self.kyoku, actor as u8) {
            return false;
        }

        let mut take_idx = self.take_idxs[actor];
        let mut discard_idx = self.discard_idxs[actor];
        while take_idx < self.take_events[actor].len() {
            if !matches!(
                self.take_events[actor].get(take_idx),
                Some(Event::Tsumo { .. })
            ) {
                return false;
            }

            loop {
                let Some(discard) = self.discard_events[actor].get(discard_idx) else {
                    return false;
                };
                discard_idx += 1;
                match discard {
                    Event::Dahai { .. } => break,
                    Event::Nukidora { .. } => continue,
                    Event::Reach { .. } | Event::Ankan { .. } | Event::Kakan { .. } => {
                        return false;
                    }
                    _ => return false,
                }
            }
            take_idx += 1;
        }

        true
    }

    fn push_dora(&mut self) -> Result<()> {
        let dora_marker = *self.kyoku.dora_indicators.get(self.dora_idx).ok_or(
            ConvertError::InsufficientDoraIndicators {
                kyoku: self.kyoku.meta.kyoku_num,
                honba: self.kyoku.meta.honba,
            },
        )?;
        self.dora_idx += 1;
        self.events.push(Event::Dora { dora_marker });
        Ok(())
    }

    fn apply_take(&mut self, event: &Event) -> Result<()> {
        if !apply_take_to_hand_checked(&mut self.hand_counts[self.actor], event) {
            return Err(unexpected_state_error(self, event.clone()));
        }
        self.temporary_furiten[self.actor] = false;
        if matches!(event, Event::Pon { .. } | Event::Daiminkan { .. }) {
            self.meld_counts[self.actor] += 1;
        }
        Ok(())
    }

    fn apply_discard(&mut self, event: &Event) -> Result<()> {
        if !apply_discard_to_hand_checked(&mut self.hand_counts[self.actor], event) {
            return Err(unexpected_state_error(self, event.clone()));
        }
        if let Event::Dahai { pai, .. } = event {
            self.discarded_tiles[self.actor][pai.deaka().as_usize()] = true;
        }
        if matches!(event, Event::Ankan { .. }) {
            self.meld_counts[self.actor] += 1;
        }
        Ok(())
    }
}

fn unexpected_state_error(state: &SanmaReplayState<'_>, action: Event) -> ConvertError {
    ConvertError::UnexpectedNaki {
        action,
        last_discard: state.last_discard,
        last_actor: state.last_actor,
        kyoku: state.kyoku.meta.kyoku_num,
        honba: state.kyoku.meta.honba,
        actor: state.actor as u8,
    }
}

fn is_final_ron_discard(
    kyoku: &Kyoku,
    actor: u8,
    discard_idxs: &[usize],
    discard_events: &[Vec<Event>],
    take_idxs: &[usize],
    take_events: &[Vec<Event>],
) -> bool {
    if discard_idxs[actor as usize] < discard_events[actor as usize].len() {
        return false;
    }

    match &kyoku.end_status {
        EndStatus::Hora { details } => details.iter().any(|detail| {
            detail.target == actor
                && detail.who != detail.target
                && take_idxs[detail.who as usize] >= take_events[detail.who as usize].len()
        }),
        EndStatus::Ryukyoku { .. } => false,
    }
}

fn can_end_after_exhausted_takes(
    kyoku: &Kyoku,
    last_discard_actor: u8,
    discard_idxs: &[usize],
    discard_events: &[Vec<Event>],
    take_idxs: &[usize],
    take_events: &[Vec<Event>],
) -> bool {
    match &kyoku.end_status {
        EndStatus::Ryukyoku { .. } => true,
        EndStatus::Hora { details } => details.iter().all(|detail| {
            if detail.who == detail.target {
                return true;
            }
            detail.target == last_discard_actor
                && discard_idxs[detail.target as usize]
                    >= discard_events[detail.target as usize].len()
                && take_idxs[detail.who as usize] >= take_events[detail.who as usize].len()
        }),
    }
}

fn is_ron_target(kyoku: &Kyoku, actor: u8) -> bool {
    match &kyoku.end_status {
        EndStatus::Hora { details } => details
            .iter()
            .any(|detail| detail.target == actor && detail.who != detail.target),
        EndStatus::Ryukyoku { .. } => false,
    }
}

fn is_ron_winner(kyoku: &Kyoku, actor: u8) -> bool {
    match &kyoku.end_status {
        EndStatus::Hora { details } => details
            .iter()
            .any(|detail| detail.who == actor && detail.who != detail.target),
        EndStatus::Ryukyoku { .. } => false,
    }
}

fn is_final_ron_target_drawn_nukidora(
    kyoku: &Kyoku,
    actor: usize,
    discard_idx: usize,
    discard_events: &[Vec<Event>],
    hand_counts: &[[u8; 38]],
) -> bool {
    hand_counts[actor][t!(N).as_usize()] == 0
        && is_ron_target(kyoku, actor as u8)
        && discard_idx + 2 == discard_events[actor].len()
        && matches!(
            discard_events[actor].get(discard_idx),
            Some(Event::Nukidora { .. })
        )
        && matches!(
            discard_events[actor].get(discard_idx + 1),
            Some(Event::Dahai {
                tsumogiri: true,
                ..
            })
        )
}

fn next_actor_with_take(
    num_players: usize,
    actor: usize,
    turn_step: usize,
    take_idxs: &[usize],
    take_events: &[Vec<Event>],
) -> usize {
    let mut next = next_turn_actor(num_players, actor, turn_step);
    for _ in 0..num_players {
        if take_idxs[next] < take_events[next].len() {
            return next;
        }
        next = next_turn_actor(num_players, next, turn_step);
    }
    actor
}

const fn next_turn_actor(num_players: usize, actor: usize, turn_step: usize) -> usize {
    (actor + turn_step) % num_players
}

fn parse_takes_and_discards_to_mjai(
    actor: u8,
    num_players: usize,
    takes: &[ActionItem],
    discards: &[ActionItem],
) -> Result<(Vec<Event>, Vec<Event>)> {
    let mjai_takes = take_action_to_events(actor, num_players, takes)?;
    let mut mjai_discards = discard_action_to_events(actor, discards)?;
    finalize_discards(&mjai_takes, &mut mjai_discards);

    Ok((mjai_takes, mjai_discards))
}

/// 1. fill in possible tsumogiri pais
/// 2. skip discards of daiminkans
fn finalize_discards(takes: &[Event], discards: &mut Vec<Event>) {
    let mut di = 0;
    let mut ti = 0;
    let mut pending_nukidora = 0;
    while ti < takes.len() {
        if di >= discards.len() {
            break;
        }

        if matches!(discards[di], Event::Reach { .. }) {
            di += 1;
        }

        while matches!(discards.get(di), Some(Event::Nukidora { .. })) {
            if !matches!(takes[ti], Event::Tsumo { pai, .. } if pai == t!(N)) {
                pending_nukidora += 1;
            }
            di += 1;
            if di >= discards.len() {
                return;
            }
            if matches!(takes[ti], Event::Tsumo { pai, .. } if pai == t!(N)) {
                ti += 1;
                if ti >= takes.len() {
                    return;
                }
            }
        }

        if matches!(discards.get(di), Some(Event::Reach { .. })) {
            di += 1;
            if di >= discards.len() {
                return;
            }
        }

        if pending_nukidora > 0 && matches!(takes[ti], Event::Tsumo { .. }) {
            pending_nukidora -= 1;
            ti += 1;
            continue;
        }

        if let Event::Dahai {
            pai,
            tsumogiri,
            actor,
        } = discards[di]
        {
            if tsumogiri {
                if let Event::Tsumo { pai: tsumo, .. } = takes[ti] {
                    discards[di] = Event::Dahai {
                        pai: tsumo,
                        tsumogiri,
                        actor,
                    }
                }
            } else if pai == t!(?) {
                // `take` is daiminkan, skip one discard and immediately consume
                // the next take.
                discards.remove(di);
                ti += 1;
                continue;
            }
        };

        di += 1;
        ti += 1;
    }
}

fn apply_take_to_hand(hand: &mut [u8; 38], event: &Event) {
    match event {
        Event::Tsumo { pai, .. } => hand[pai.as_usize()] += 1,
        Event::Pon { consumed, .. } => {
            for pai in consumed {
                hand[pai.as_usize()] = hand[pai.as_usize()].saturating_sub(1);
            }
        }
        Event::Daiminkan { consumed, .. } => {
            for pai in consumed {
                hand[pai.as_usize()] = hand[pai.as_usize()].saturating_sub(1);
            }
        }
        _ => (),
    }
}

fn apply_take_to_hand_checked(hand: &mut [u8; 38], event: &Event) -> bool {
    let mut next = *hand;
    match event {
        Event::Tsumo { pai, .. } => next[pai.as_usize()] += 1,
        Event::Pon { consumed, .. } => {
            for pai in consumed {
                if !remove_from_hand(&mut next, *pai) {
                    return false;
                }
            }
        }
        Event::Daiminkan { consumed, .. } => {
            for pai in consumed {
                if !remove_from_hand(&mut next, *pai) {
                    return false;
                }
            }
        }
        _ => (),
    }
    *hand = next;
    true
}

fn apply_discard_to_hand(hand: &mut [u8; 38], event: &Event) {
    match event {
        Event::Dahai { pai, .. } | Event::Kakan { pai, .. } | Event::Nukidora { pai, .. } => {
            hand[pai.as_usize()] = hand[pai.as_usize()].saturating_sub(1);
        }
        Event::Ankan { consumed, .. } => {
            for pai in consumed {
                hand[pai.as_usize()] = hand[pai.as_usize()].saturating_sub(1);
            }
        }
        _ => (),
    }
}

fn apply_discard_to_hand_checked(hand: &mut [u8; 38], event: &Event) -> bool {
    let mut next = *hand;
    match event {
        Event::Dahai { pai, .. } | Event::Kakan { pai, .. } | Event::Nukidora { pai, .. } => {
            if !remove_from_hand(&mut next, *pai) {
                return false;
            }
        }
        Event::Ankan { consumed, .. } => {
            for pai in consumed {
                if !remove_from_hand(&mut next, *pai) {
                    return false;
                }
            }
        }
        _ => (),
    }
    *hand = next;
    true
}

fn remove_from_hand(hand: &mut [u8; 38], pai: Tile) -> bool {
    let count = &mut hand[pai.as_usize()];
    if *count == 0 {
        return false;
    }
    *count -= 1;
    true
}

fn is_complete_hand_shape_with_win_tile(hand: &[u8; 38], win_tile: Tile, meld_count: u8) -> bool {
    if meld_count > 4 || win_tile.is_unknown() {
        return false;
    }

    let mut counts = normalized_tile_counts(hand);
    counts[win_tile.deaka().as_usize()] += 1;
    let tile_count: u8 = counts.iter().sum();
    if tile_count != 14 - meld_count * 3 {
        return false;
    }

    if meld_count == 0 && (is_kokushi_shape(&counts) || is_chiitoitsu_shape(&counts)) {
        return true;
    }

    is_standard_hand_shape(&mut counts, 4 - meld_count)
}

fn is_tenpai_hand_shape(hand: &[u8; 38], meld_count: u8) -> bool {
    let counts = normalized_tile_counts(hand);
    (0..34).any(|tile_id| {
        counts[tile_id] < 4
            && is_complete_hand_shape_with_win_tile(
                hand,
                Tile::try_from(tile_id).expect("valid tile id"),
                meld_count,
            )
    })
}

fn is_tanyao_shape_with_win_tile(hand: &[u8; 38], win_tile: Tile) -> bool {
    let mut counts = normalized_tile_counts(hand);
    counts[win_tile.deaka().as_usize()] += 1;
    ![
        t!(1m),
        t!(9m),
        t!(1p),
        t!(9p),
        t!(1s),
        t!(9s),
        t!(E),
        t!(S),
        t!(W),
        t!(N),
        t!(P),
        t!(F),
        t!(C),
    ]
    .into_iter()
    .any(|tile| counts[tile.as_usize()] > 0)
}

fn normalized_tile_counts(hand: &[u8; 38]) -> [u8; 34] {
    let mut counts = [0; 34];
    counts.copy_from_slice(&hand[..34]);
    counts[t!(5m).as_usize()] += hand[t!(5mr).as_usize()];
    counts[t!(5p).as_usize()] += hand[t!(5pr).as_usize()];
    counts[t!(5s).as_usize()] += hand[t!(5sr).as_usize()];
    counts
}

fn is_kokushi_shape(counts: &[u8; 34]) -> bool {
    let terminals = [
        t!(1m),
        t!(9m),
        t!(1p),
        t!(9p),
        t!(1s),
        t!(9s),
        t!(E),
        t!(S),
        t!(W),
        t!(N),
        t!(P),
        t!(F),
        t!(C),
    ];
    let mut has_pair = false;
    for tile in terminals {
        match counts[tile.as_usize()] {
            1 => (),
            2 if !has_pair => has_pair = true,
            _ => return false,
        }
    }
    has_pair
}

fn is_chiitoitsu_shape(counts: &[u8; 34]) -> bool {
    counts.iter().filter(|&&count| count == 2).count() == 7
        && counts.iter().all(|&count| count == 0 || count == 2)
}

fn is_standard_hand_shape(counts: &mut [u8; 34], melds_needed: u8) -> bool {
    for pair in 0..34 {
        if counts[pair] < 2 {
            continue;
        }
        counts[pair] -= 2;
        if can_remove_mentsu(counts, melds_needed) {
            counts[pair] += 2;
            return true;
        }
        counts[pair] += 2;
    }
    false
}

fn can_remove_mentsu(counts: &mut [u8; 34], melds_left: u8) -> bool {
    let Some(tile) = counts.iter().position(|&count| count > 0) else {
        return melds_left == 0;
    };
    if melds_left == 0 {
        return false;
    }

    if counts[tile] >= 3 {
        counts[tile] -= 3;
        if can_remove_mentsu(counts, melds_left - 1) {
            counts[tile] += 3;
            return true;
        }
        counts[tile] += 3;
    }

    let suit = tile / 9;
    let number = tile % 9;
    if suit < 3 && number <= 6 && counts[tile + 1] > 0 && counts[tile + 2] > 0 {
        counts[tile] -= 1;
        counts[tile + 1] -= 1;
        counts[tile + 2] -= 1;
        if can_remove_mentsu(counts, melds_left - 1) {
            counts[tile] += 1;
            counts[tile + 1] += 1;
            counts[tile + 2] += 1;
            return true;
        }
        counts[tile] += 1;
        counts[tile + 1] += 1;
        counts[tile + 2] += 1;
    }

    false
}

fn take_action_to_events(
    actor: u8,
    num_players: usize,
    takes: &[ActionItem],
) -> Result<Vec<Event>> {
    takes
        .iter()
        .map(|take| match take {
            ActionItem::Tsumogiri(_) => Err(ConvertError::UnexpectedTsumogiri),
            &ActionItem::Tile(pai) => Ok(Event::Tsumo { actor, pai }),
            ActionItem::Naki(naki_string) => {
                let naki = naki_string.as_bytes();

                if naki.contains(&b'c') {
                    // chi
                    // you can only chi from kamicha right...?
                    if num_players == 3 {
                        return Err(ConvertError::SanmaChi);
                    }

                    if naki_string.len() != 7 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    // e.g. "c275226" => chi 7p with 06p from kamicha
                    Ok(Event::Chi {
                        actor,
                        target: relative_target(actor, num_players, 0),
                        pai: tiles_from_tenhou_bytes(&naki[1..3])?,
                        consumed: [
                            tiles_from_tenhou_bytes(&naki[3..5])?,
                            tiles_from_tenhou_bytes(&naki[5..7])?,
                        ],
                    })
                } else if let Some(idx) = naki_string.find('p') {
                    // pon

                    if naki_string.len() != 7 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    match idx {
                        // from kamicha
                        // e.g. "p252525" => pon 5p from kamicha
                        0 => Ok(Event::Pon {
                            actor,
                            target: relative_target(actor, num_players, 0),
                            pai: tiles_from_tenhou_bytes(&naki[1..3])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[3..5])?,
                                tiles_from_tenhou_bytes(&naki[5..7])?,
                            ],
                        }),

                        // from toimen
                        // e.g. "12p1212" => pon 2m from toimen
                        2 => Ok(Event::Pon {
                            actor,
                            target: relative_target(actor, num_players, 2),
                            pai: tiles_from_tenhou_bytes(&naki[3..5])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[0..2])?,
                                tiles_from_tenhou_bytes(&naki[5..7])?,
                            ],
                        }),

                        // from shimocha
                        // e.g. "3737p37" => pon 7s from shimocha
                        4 => Ok(Event::Pon {
                            actor,
                            target: relative_target(actor, num_players, 4),
                            pai: tiles_from_tenhou_bytes(&naki[5..7])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[0..2])?,
                                tiles_from_tenhou_bytes(&naki[2..4])?,
                            ],
                        }),

                        // ???
                        _ => Err(ConvertError::InvalidNaki(naki_string.clone())),
                    }
                } else if let Some(idx) = naki_string.find('m') {
                    // daiminkan

                    if naki_string.len() != 9 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    match idx {
                        // from kamicha
                        // e.g. "m39393939" => kan 9s from kamicha
                        0 => Ok(Event::Daiminkan {
                            actor,
                            target: relative_target(actor, num_players, 0),
                            pai: tiles_from_tenhou_bytes(&naki[1..3])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[3..5])?,
                                tiles_from_tenhou_bytes(&naki[5..7])?,
                                tiles_from_tenhou_bytes(&naki[7..9])?,
                            ],
                        }),

                        // from toimen
                        // e.g. "26m262626" => kan 6p from toimen
                        2 => Ok(Event::Daiminkan {
                            actor,
                            target: relative_target(actor, num_players, 2),
                            pai: tiles_from_tenhou_bytes(&naki[3..5])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[0..2])?,
                                tiles_from_tenhou_bytes(&naki[5..7])?,
                                tiles_from_tenhou_bytes(&naki[7..9])?,
                            ],
                        }),

                        // from shimocha
                        // e.g. "131313m13" => kan 3m from shimocha
                        6 => Ok(Event::Daiminkan {
                            actor,
                            target: relative_target(actor, num_players, 6),
                            pai: tiles_from_tenhou_bytes(&naki[7..9])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[0..2])?,
                                tiles_from_tenhou_bytes(&naki[2..4])?,
                                tiles_from_tenhou_bytes(&naki[4..6])?,
                            ],
                        }),

                        // ???
                        _ => Err(ConvertError::InvalidNaki(naki_string.clone())),
                    }
                } else {
                    Err(ConvertError::InvalidNaki(naki_string.clone()))
                }
            }
        })
        .collect()
}

fn discard_action_to_events(actor: u8, discards: &[ActionItem]) -> Result<Vec<Event>> {
    let mut ret = vec![];

    for discard in discards {
        match discard {
            &ActionItem::Tile(pai) => {
                let ev = Event::Dahai {
                    actor,
                    pai,
                    tsumogiri: false,
                };

                ret.push(ev);
            }

            ActionItem::Tsumogiri(_) => {
                let ev = Event::Dahai {
                    actor,
                    pai: t!(?), // must be filled later
                    tsumogiri: true,
                };

                ret.push(ev);
            }

            ActionItem::Naki(naki_string) => {
                let naki = naki_string.as_bytes();

                // only ankan, kakan and reach are possible
                if let Some(idx) = naki_string.find('k') {
                    // kakan

                    if naki_string.len() != 9 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    let ev = match idx {
                        // previously pon from toimen
                        // e.g. "k16161616" => pon 6m from kamicha then kan
                        0 => Event::Kakan {
                            actor,
                            pai: tiles_from_tenhou_bytes(&naki[1..3])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[3..5])?,
                                tiles_from_tenhou_bytes(&naki[5..7])?,
                                tiles_from_tenhou_bytes(&naki[7..9])?,
                            ],
                        },

                        // previously pon from toimen
                        // e.g. "41k414141" => pon 1z from toimen then kan
                        2 => Event::Kakan {
                            actor,
                            pai: tiles_from_tenhou_bytes(&naki[3..5])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[0..2])?,
                                tiles_from_tenhou_bytes(&naki[5..7])?,
                                tiles_from_tenhou_bytes(&naki[7..9])?,
                            ],
                        },

                        // previously pon from shimocha
                        // e.g. "4646k4646" => pon 6z from shimocha then kan
                        4 => Event::Kakan {
                            actor,
                            pai: tiles_from_tenhou_bytes(&naki[5..7])?,
                            consumed: [
                                tiles_from_tenhou_bytes(&naki[0..2])?,
                                tiles_from_tenhou_bytes(&naki[2..4])?,
                                tiles_from_tenhou_bytes(&naki[7..9])?,
                            ],
                        },

                        // ???
                        _ => {
                            return Err(ConvertError::InvalidNaki(naki_string.clone()));
                        }
                    };

                    ret.push(ev);
                } else if naki.contains(&b'a') {
                    // ankan
                    // for ankan, 'a' can only appear at [6]
                    // e.g. "424242a42" => ankan 2z

                    if naki_string.len() != 9 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    let pai = tiles_from_tenhou_bytes(&naki[7..9])?;
                    let ev = Event::Ankan {
                        actor,
                        consumed: [
                            tiles_from_tenhou_bytes(&naki[0..2])?,
                            tiles_from_tenhou_bytes(&naki[2..4])?,
                            tiles_from_tenhou_bytes(&naki[4..6])?,
                            pai,
                        ],
                    };

                    ret.push(ev);
                } else if naki.contains(&b'f') {
                    // Tenhou sanma represents kita / nuki-dora as "f44".
                    if naki_string != "f44" {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    ret.push(Event::Nukidora { actor, pai: t!(N) });
                } else {
                    // reach
                    // e.g. "r35" => discard 5s to reach

                    if naki_string.len() != 3 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    let pai = if &naki[1..3] == b"60" {
                        t!(?)
                    } else {
                        tiles_from_tenhou_bytes(&naki[1..3])?
                    };

                    ret.push(Event::Reach { actor });
                    ret.push(Event::Dahai {
                        actor,
                        pai, // must be filled later if it is tsumogiri
                        tsumogiri: pai == t!(?),
                    });
                }
            }
        };
    }

    Ok(ret)
}

fn end_kyoku(events: &mut Vec<Event>, kyoku: &Kyoku) {
    match &kyoku.end_status {
        EndStatus::Hora { details } => {
            events.extend(details.iter().map(|detail| Event::Hora {
                actor: detail.who,
                target: detail.target,
                deltas: Some(detail.score_deltas.clone()),
                ura_markers: Some(kyoku.ura_indicators.clone()),
            }));
        }

        EndStatus::Ryukyoku { score_deltas } => {
            events.push(Event::Ryukyoku {
                deltas: Some(score_deltas.clone()),
            });
        }
    };

    events.push(Event::EndKyoku);
}

pub fn tiles_from_tenhou_bytes(b: &[u8]) -> Result<Tile> {
    let s = std::str::from_utf8(b)
        .map_err(|_| ConvertError::InvalidTile(String::from_utf8_lossy(b).into_owned()))?;
    let id: u8 = s
        .parse()
        .map_err(|_| ConvertError::InvalidTile(s.to_owned()))?;
    let tenhou_tile =
        TenhouTile::try_from(id).map_err(|_| ConvertError::InvalidTile(s.to_owned()))?;
    Ok(Tile::from(tenhou_tile))
}

const fn relative_target(actor: u8, num_players: usize, naki_marker_idx: usize) -> u8 {
    match (num_players, naki_marker_idx) {
        (4, 0) => (actor + 3) % 4,
        (4, 2) => (actor + 2) % 4,
        (4, 4 | 6) => (actor + 1) % 4,
        // Sanma has no kamicha seat, but Tenhou still uses the same compact
        // meld-string forms. Indices before the called tile map to the previous
        // actor in turn order; indices after it map to the next actor.
        (3, 0) => (actor + 2) % 3,
        (3, 2 | 4 | 6) => (actor + 1) % 3,
        _ => actor,
    }
}

fn retarget_naki(event: &mut Event, target: u8) {
    match event {
        Event::Pon { target: t, .. } | Event::Daiminkan { target: t, .. } => *t = target,
        _ => (),
    }
}

fn validate_sanma_mjai(events: &[Event]) -> Result<()> {
    for event in events {
        if matches!(event, Event::Chi { .. }) {
            return Err(ConvertError::SanmaChi);
        }
        if event.actor().is_some_and(|actor| actor >= 3) {
            return Err(ConvertError::UnexpectedNaki {
                action: event.clone(),
                last_discard: t!(?),
                last_actor: None,
                kyoku: 0,
                honba: 0,
                actor: 3,
            });
        }
        validate_sanma_event_tiles(event)?;
    }

    Ok(())
}

fn validate_sanma_event_tiles(event: &Event) -> Result<()> {
    match event {
        Event::StartKyoku {
            dora_marker,
            scores,
            tehais,
            ..
        } => {
            if scores.len() != 3 || tehais.len() != 3 {
                return Err(ConvertError::UnexpectedNaki {
                    action: event.clone(),
                    last_discard: t!(?),
                    last_actor: None,
                    kyoku: 0,
                    honba: 0,
                    actor: 3,
                });
            }
            validate_sanma_tile(*dora_marker)?;
            for pai in tehais.iter().flatten() {
                validate_sanma_tile(*pai)?;
            }
        }
        Event::StartGame { names, .. } if names.len() != 3 => {
            return Err(ConvertError::UnexpectedNaki {
                action: event.clone(),
                last_discard: t!(?),
                last_actor: None,
                kyoku: 0,
                honba: 0,
                actor: 3,
            });
        }
        Event::Tsumo { pai, .. }
        | Event::Dahai { pai, .. }
        | Event::Nukidora { pai, .. }
        | Event::Kakan { pai, .. }
        | Event::Dora { dora_marker: pai } => validate_sanma_tile(*pai)?,
        Event::Chi { pai, consumed, .. } | Event::Pon { pai, consumed, .. } => {
            validate_sanma_tile(*pai)?;
            for pai in consumed {
                validate_sanma_tile(*pai)?;
            }
        }
        Event::Daiminkan { pai, consumed, .. } => {
            validate_sanma_tile(*pai)?;
            for pai in consumed {
                validate_sanma_tile(*pai)?;
            }
        }
        Event::Ankan { consumed, .. } => {
            for pai in consumed {
                validate_sanma_tile(*pai)?;
            }
        }
        Event::Hora {
            ura_markers: Some(ura_markers),
            ..
        } => {
            for pai in ura_markers {
                validate_sanma_tile(*pai)?;
            }
        }
        _ => (),
    }

    Ok(())
}

fn validate_sanma_tile(tile: Tile) -> Result<()> {
    match tile.as_u8() {
        1..=7 | 34 => Err(ConvertError::InvalidSanmaTile(tile)),
        _ => Ok(()),
    }
}
