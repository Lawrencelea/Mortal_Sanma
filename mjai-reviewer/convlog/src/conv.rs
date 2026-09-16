use crate::Tile;
use crate::mjai::Event;
use crate::t;
use crate::tenhou::{ActionItem, EndStatus, Kyoku, Log, TenhouTile};
use std::collections::hash_map::Entry;

use ahash::AHashMap;
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
        "no physically valid event order for sanma kyoku: \
        at kyoku {kyoku} honba {honba}"
    )]
    NoValidSanmaOrder { kyoku: u8, honba: u8 },

    #[error(
        "ambiguous event order for sanma kyoku: \
        at kyoku {kyoku} honba {honba}"
    )]
    AmbiguousSanmaOrder { kyoku: u8, honba: u8 },

    #[error("insufficient dora indicators: at kyoku {kyoku} honba {honba}")]
    InsufficientDoraIndicators { kyoku: u8, honba: u8 },

    #[error(
        "{unconsumed} dora indicator(s) were never revealed by the replay: \
        at kyoku {kyoku} honba {honba}"
    )]
    UnconsumedDoraIndicators {
        kyoku: u8,
        honba: u8,
        unconsumed: usize,
    },

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
    let start_kyoku = Event::StartKyoku {
        bakaze,
        kyoku: kyoku.meta.kyoku_num % 4 + 1,
        honba: kyoku.meta.honba,
        kyotaku: kyoku.meta.kyotaku,
        dora_marker: *kyoku
            .dora_indicators
            .first()
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
    };

    if num_players == 3 {
        tenhou_sanma_kyoku_to_mjai_events(kyoku, start_kyoku, oya)
    } else {
        tenhou_yonma_kyoku_to_mjai_events(kyoku, start_kyoku, oya)
    }
}

// ---------------------------------------------------------------------------
// Four-player conversion. This is the upstream mjai-reviewer algorithm,
// unchanged except for the `Vec`-based log types.
// ---------------------------------------------------------------------------

fn tenhou_yonma_kyoku_to_mjai_events(
    kyoku: &Kyoku,
    start_kyoku: Event,
    oya: u8,
) -> Result<Vec<Event>> {
    // First of all, transform all takes and discards to events.
    let (take_events, discard_events): (Vec<_>, Vec<_>) = (0..4)
        .map(|a| {
            parse_takes_and_discards_to_mjai(
                a,
                4,
                &kyoku.action_tables[a as usize].takes,
                &kyoku.action_tables[a as usize].discards,
            )
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .unzip();

    // Prepare for backtracks.
    let mut backtracks = AHashMap::new();

    let attempt = |backtracks: &mut AHashMap<Tile, BackTrack>| -> Result<Vec<Event>> {
        let mut events = vec![start_kyoku.clone()];
        let mut dora_feed = kyoku.dora_indicators.iter().copied().skip(1);

        let mut discard_sets: Vec<_> = (0..4)
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
        let mut take_idxs = [0; 4];
        let mut discard_idxs = [0; 4];

        let mut reach_flag: Option<usize> = None;
        let mut last_discard = t!(?);
        let mut last_actor: Option<u8> = None;
        let mut need_new_dora_at_discard = false;
        // This is for Kakan only because chankan is possible until an actual
        // tsumo.
        let mut need_new_dora_at_tsumo = false;

        let mut actor = oya as usize;

        loop {
            // Start to process a take event.
            let take = take_events[actor].get(take_idxs[actor]).ok_or(
                ConvertError::InsufficientTakes {
                    kyoku: kyoku.meta.kyoku_num,
                    honba: kyoku.meta.honba,
                    actor: actor as u8,
                },
            )?;
            take_idxs[actor] += 1;

            if let Some((target, pai)) = take.naki_info() {
                if pai != last_discard
                    || last_actor.is_some_and(|a| a != target || a == actor as u8)
                {
                    return Err(ConvertError::UnexpectedNaki {
                        action: take.clone(),
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
            match *take {
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

                    events.push(take.clone());
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
                events.push(dahai);
            }

            // Check if the kyoku ends here, can be ryukyoku or ron.
            //
            // Here it simply checks if there is no more take for every single
            // actor.
            if (0..4).all(|a| take_idxs[a] >= take_events[a].len()) {
                end_kyoku(&mut events, kyoku);
                break;
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
            actor = (0..4)
                .filter(|&a| a != actor)
                // First pass, filter the naki that takes the specific tile from the
                // specific target.
                .filter_map(|a| {
                    if let Some(take) = take_events[a].get(take_idxs[a]) {
                        if let Some((target, pai)) = take.naki_info() {
                            if target == (actor as u8) && pai == last_discard {
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
                })
                .unwrap_or((actor + 1) % 4);
        }

        Ok(events)
    };

    let mut first_error = None;
    loop {
        match attempt(&mut backtracks) {
            Ok(events) => return Ok(events),
            Err(err) => {
                first_error = first_error.or(Some(err));
                if backtracks.is_empty() {
                    return Err(first_error.unwrap());
                }
            }
        };
    }
}

// ---------------------------------------------------------------------------
// Three-player conversion.
//
// Tenhou records one take stream and one discard stream per player. The
// streams do not carry timing information, so the converter must interleave
// them. The interleaving is fully determined by the physical rules of the
// game, and nothing else is assumed:
//
//   * A player who is on turn draws the next item of their take stream, which
//     must be a plain draw, then plays the next item of their discard stream.
//   * After a discard of tile T by X, either some player Y whose next take is
//     "call T from X" makes that call now, or X's shimocha draws. Both options
//     are explored; declining a call is legal, but it must lead to a
//     consistent replay of everyone's streams.
//   * Nukidora, ankan, kakan and daiminkan are followed by a replacement draw
//     of the same player.
//   * The kyoku ends exactly when every stream is fully consumed and the last
//     event is compatible with the recorded result (tsumo by the winner, or a
//     discard-like event by the ron target).
//
// The search records every complete interleaving. A kyoku is converted only
// when exactly one exists, so no guess is ever baked into the output.
// ---------------------------------------------------------------------------

fn tenhou_sanma_kyoku_to_mjai_events(
    kyoku: &Kyoku,
    start_kyoku: Event,
    oya: u8,
) -> Result<Vec<Event>> {
    let mut takes = Vec::with_capacity(3);
    let mut discards = Vec::with_capacity(3);
    for (actor, table) in kyoku.action_tables.iter().enumerate() {
        takes.push(take_action_to_events(actor as u8, 3, &table.takes)?);
        discards.push(discard_action_to_events(actor as u8, &table.discards)?);
    }

    let mut replay = SanmaReplay {
        kyoku,
        takes: &takes,
        discards: &discards,
        events: vec![start_kyoku],
        take_idxs: [0; 3],
        discard_idxs: [0; 3],
        last_draw: [t!(?); 3],
        dora_idx: 1,
        reach_pending: None,
        need_dora_at_discard: false,
        need_dora_at_tsumo: false,
        solutions: vec![],
    };
    replay.draw_phase(oya as usize, Last::Other)?;

    match replay.solutions.len() {
        0 => Err(ConvertError::NoValidSanmaOrder {
            kyoku: kyoku.meta.kyoku_num,
            honba: kyoku.meta.honba,
        }),
        1 => {
            let events = replay.solutions.pop().unwrap();
            // Tenhou lists exactly the indicators that were revealed, so a
            // correct replay consumes all of them. Anything left over means
            // a dora was emitted too late (or not at all).
            let revealed = 1 + events
                .iter()
                .filter(|event| matches!(event, Event::Dora { .. }))
                .count();
            if revealed < kyoku.dora_indicators.len() {
                return Err(ConvertError::UnconsumedDoraIndicators {
                    kyoku: kyoku.meta.kyoku_num,
                    honba: kyoku.meta.honba,
                    unconsumed: kyoku.dora_indicators.len() - revealed,
                });
            }
            Ok(events)
        }
        _ => Err(ConvertError::AmbiguousSanmaOrder {
            kyoku: kyoku.meta.kyoku_num,
            honba: kyoku.meta.honba,
        }),
    }
}

/// What the most recent event was, for deciding whether the kyoku may end
/// here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    /// A draw by this actor; only a tsumo win can follow.
    Draw(usize),
    /// A dahai, kakan or nukidora by this actor; only a ron on this actor can
    /// follow.
    Discard(usize),
    /// Anything else; only ryukyoku can follow.
    Other,
}

struct SanmaReplay<'a> {
    kyoku: &'a Kyoku,
    takes: &'a [Vec<Event>],
    discards: &'a [Vec<Event>],
    events: Vec<Event>,
    take_idxs: [usize; 3],
    discard_idxs: [usize; 3],
    last_draw: [Tile; 3],
    dora_idx: usize,
    reach_pending: Option<u8>,
    need_dora_at_discard: bool,
    need_dora_at_tsumo: bool,
    solutions: Vec<Vec<Event>>,
}

#[derive(Clone, Copy)]
struct Snapshot {
    events_len: usize,
    take_idxs: [usize; 3],
    discard_idxs: [usize; 3],
    last_draw: [Tile; 3],
    dora_idx: usize,
    reach_pending: Option<u8>,
    need_dora_at_discard: bool,
    need_dora_at_tsumo: bool,
}

impl SanmaReplay<'_> {
    const MAX_SOLUTIONS: usize = 2;

    const fn snapshot(&self) -> Snapshot {
        Snapshot {
            events_len: self.events.len(),
            take_idxs: self.take_idxs,
            discard_idxs: self.discard_idxs,
            last_draw: self.last_draw,
            dora_idx: self.dora_idx,
            reach_pending: self.reach_pending,
            need_dora_at_discard: self.need_dora_at_discard,
            need_dora_at_tsumo: self.need_dora_at_tsumo,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.events.truncate(snapshot.events_len);
        self.take_idxs = snapshot.take_idxs;
        self.discard_idxs = snapshot.discard_idxs;
        self.last_draw = snapshot.last_draw;
        self.dora_idx = snapshot.dora_idx;
        self.reach_pending = snapshot.reach_pending;
        self.need_dora_at_discard = snapshot.need_dora_at_discard;
        self.need_dora_at_tsumo = snapshot.need_dora_at_tsumo;
    }

    const fn done(&self) -> bool {
        self.solutions.len() >= Self::MAX_SOLUTIONS
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

    fn accept_pending_reach(&mut self) {
        if let Some(actor) = self.reach_pending.take() {
            self.events.push(Event::ReachAccepted { actor });
        }
    }

    fn all_consumed(&self) -> bool {
        (0..3).all(|a| {
            self.take_idxs[a] >= self.takes[a].len()
                && self.discard_idxs[a] >= self.discards[a].len()
        })
    }

    /// Record a solution if every stream is consumed and the recorded result
    /// can follow the last event.
    fn try_end(&mut self, last: Last) {
        if self.done() || !self.all_consumed() {
            return;
        }

        let consistent = match &self.kyoku.end_status {
            EndStatus::Ryukyoku { .. } => true,
            EndStatus::Hora { details } => details.iter().all(|detail| {
                if detail.who == detail.target {
                    last == Last::Draw(detail.who as usize)
                } else {
                    last == Last::Discard(detail.target as usize)
                }
            }),
        };
        if !consistent {
            return;
        }

        let mut events = self.events.clone();
        // A riichi declared on the very last discard is still accepted by
        // Tenhou (the 1000-point deposit is taken) when the hand then ends in
        // any ryukyoku; only a ron on that discard cancels it.
        if let (Some(actor), EndStatus::Ryukyoku { .. }) = (self.reach_pending, &self.kyoku.end_status) {
            events.push(Event::ReachAccepted { actor });
        }
        end_kyoku(&mut events, self.kyoku);
        self.solutions.push(events);
    }

    /// `actor` is on turn and must draw.
    fn draw_phase(&mut self, actor: usize, last: Last) -> Result<()> {
        if self.done() {
            return Ok(());
        }

        let Some(take) = self.takes[actor].get(self.take_idxs[actor]) else {
            self.try_end(last);
            return Ok(());
        };
        let Event::Tsumo { pai, .. } = *take else {
            // A call cannot happen on one's own turn.
            return Ok(());
        };
        self.take_idxs[actor] += 1;

        self.accept_pending_reach();
        if self.need_dora_at_tsumo {
            self.push_dora()?;
            self.need_dora_at_tsumo = false;
        }
        self.events.push(Event::Tsumo {
            actor: actor as u8,
            pai,
        });
        self.last_draw[actor] = pai;

        self.discard_phase(actor)
    }

    /// `actor` holds 14 tiles (after a draw or a pon) and must act.
    fn discard_phase(&mut self, actor: usize) -> Result<()> {
        if self.done() {
            return Ok(());
        }

        let Some(item) = self.discards[actor].get(self.discard_idxs[actor]).cloned() else {
            // No action recorded: tsumo agari or an abortive draw.
            self.try_end(Last::Draw(actor));
            return Ok(());
        };

        match item {
            Event::Nukidora { .. } => {
                self.discard_idxs[actor] += 1;
                // Tenhou reveals a pending minkan / kakan dora once the kita
                // passes the ron window, i.e. before the replacement draw.
                if self.need_dora_at_discard {
                    self.need_dora_at_discard = false;
                    self.need_dora_at_tsumo = true;
                }
                self.events.push(item);
                self.draw_phase(actor, Last::Discard(actor))
            }
            Event::Ankan { .. } => {
                self.discard_idxs[actor] += 1;
                if self.need_dora_at_discard {
                    self.push_dora()?;
                    self.need_dora_at_discard = false;
                }
                self.events.push(item);
                self.push_dora()?;
                self.draw_phase(actor, Last::Other)
            }
            Event::Kakan { .. } => {
                self.discard_idxs[actor] += 1;
                // The dora of a previous minkan is still pending; chankan is
                // possible on this kakan, so both are revealed at the next
                // draw / discard.
                if self.need_dora_at_discard {
                    self.need_dora_at_tsumo = true;
                }
                self.need_dora_at_discard = true;
                self.events.push(item);
                self.draw_phase(actor, Last::Discard(actor))
            }
            Event::Reach { .. } => {
                self.discard_idxs[actor] += 1;
                self.events.push(item);
                self.reach_pending = Some(actor as u8);
                let dahai = self.discards[actor]
                    .get(self.discard_idxs[actor])
                    .cloned()
                    .ok_or(ConvertError::InsufficientDiscards {
                        kyoku: self.kyoku.meta.kyoku_num,
                        honba: self.kyoku.meta.honba,
                        actor: actor as u8,
                    })?;
                self.dahai(actor, dahai)
            }
            Event::Dahai { .. } => self.dahai(actor, item),
            _ => Ok(()),
        }
    }

    fn dahai(&mut self, actor: usize, item: Event) -> Result<()> {
        let Event::Dahai { pai, tsumogiri, .. } = item else {
            return Ok(());
        };
        let pai = if tsumogiri { self.last_draw[actor] } else { pai };
        if pai.is_unknown() {
            // Either a tsumogiri with no preceding draw, or a stray daiminkan
            // placeholder; neither is a legal continuation.
            return Ok(());
        }
        self.discard_idxs[actor] += 1;

        if self.need_dora_at_discard {
            self.push_dora()?;
            self.need_dora_at_discard = false;
        }
        self.events.push(Event::Dahai {
            actor: actor as u8,
            pai,
            tsumogiri,
        });

        self.after_discard(actor, pai)
    }

    /// `actor` has just discarded `pai`. Explore every legal continuation.
    fn after_discard(&mut self, actor: usize, pai: Tile) -> Result<()> {
        let snapshot = self.snapshot();

        // Option 1: someone calls the tile.
        for caller in (0..3).filter(|&a| a != actor) {
            if self.done() {
                return Ok(());
            }
            let Some(take) = self.takes[caller].get(self.take_idxs[caller]) else {
                continue;
            };
            let Some((target, called)) = take.naki_info() else {
                continue;
            };
            if target != actor as u8 || called.deaka() != pai.deaka() {
                continue;
            }
            let take = take.clone();

            self.take_idxs[caller] += 1;
            self.accept_pending_reach();
            match take {
                Event::Pon { .. } => {
                    self.events.push(take);
                    self.discard_phase(caller)?;
                }
                Event::Daiminkan { .. } => {
                    if self.need_dora_at_discard {
                        self.push_dora()?;
                    }
                    self.events.push(take);
                    self.need_dora_at_discard = true;
                    // Tenhou leaves a placeholder in the discard stream for
                    // the missing discard of a daiminkan.
                    if matches!(
                        self.discards[caller].get(self.discard_idxs[caller]),
                        Some(Event::Dahai { pai, tsumogiri: false, .. }) if pai.is_unknown()
                    ) {
                        self.discard_idxs[caller] += 1;
                    }
                    self.draw_phase(caller, Last::Other)?;
                }
                _ => (),
            }
            self.restore(snapshot);
        }

        if self.done() {
            return Ok(());
        }

        // Option 2: nobody calls and the shimocha draws.
        let shimocha = (actor + 1) % 3;
        if matches!(
            self.takes[shimocha].get(self.take_idxs[shimocha]),
            Some(Event::Tsumo { .. })
        ) {
            self.draw_phase(shimocha, Last::Discard(actor))?;
            self.restore(snapshot);
        } else {
            // Option 3: nobody can act, so the kyoku ends on this discard.
            self.try_end(Last::Discard(actor));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared parsing helpers.
// ---------------------------------------------------------------------------

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

/// Four-player only:
/// 1. fill in possible tsumogiri pais
/// 2. skip discards of daiminkans
fn finalize_discards(takes: &[Event], discards: &mut Vec<Event>) {
    let mut di = 0;
    for take in takes {
        if di >= discards.len() {
            break;
        }

        if matches!(discards[di], Event::Reach { .. }) {
            di += 1;
        }

        if let Event::Dahai {
            pai,
            tsumogiri,
            actor,
        } = discards[di]
        {
            if tsumogiri {
                if let Event::Tsumo { pai: tsumo, .. } = *take {
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
                continue;
            }
        };

        di += 1;
    }
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

                // only ankan, kakan, nukidora and reach are possible
                if let Some(idx) = naki_string.find('k') {
                    // kakan

                    if naki_string.len() != 9 {
                        return Err(ConvertError::InvalidNaki(naki_string.clone()));
                    }

                    let ev = match idx {
                        // previously pon from kamicha
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
        (4, 4 | 6) => (actor + 1) % 4,
        // Sanma has no N seat, but Tenhou still uses the same compact meld-string
        // forms. Index 0 calls from kamicha, index 4/6 from shimocha.
        (3, 0) => (actor + 2) % 3,
        (3, 4 | 6) => (actor + 1) % 3,
        // Index 2 is the four-player toimen formula for both: in sanma it maps
        // E to W and W to E, and S never produces index 2.
        (_, 2) => (actor + 2) % 4,
        _ => actor,
    }
}

// ---------------------------------------------------------------------------
// Sanity checks on sanma output.
// ---------------------------------------------------------------------------

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

const fn validate_sanma_tile(tile: Tile) -> Result<()> {
    match tile.as_u8() {
        1..=7 | 34 => Err(ConvertError::InvalidSanmaTile(tile)),
        _ => Ok(()),
    }
}
