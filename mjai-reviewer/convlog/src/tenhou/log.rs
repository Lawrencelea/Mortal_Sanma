use super::json_scheme::{ActionItem, KyokuMeta, RawLog, ResultItem};
use crate::{KyokuFilter, Tile};

use serde::Serialize;
use serde_json::{self as json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid json: {source}")]
    InvalidJSON {
        #[from]
        source: json::Error,
    },
    #[error("unsupported player count: {0}")]
    UnsupportedPlayerCount(usize),
    #[error("invalid hora detail")]
    InvalidHoraDetail,
}

/// The overview structure of log in tenhou.net/6 format.
#[derive(Debug, Clone)]
pub struct Log {
    pub names: Vec<String>,
    pub num_players: usize,
    pub game_length: GameLength,
    pub has_aka: bool,
    pub kyokus: Vec<Kyoku>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GameLength {
    Hanchan = 0,
    Tonpuu = 4,
}

/// Contains information about a kyoku.
#[derive(Debug, Clone)]
pub struct Kyoku {
    pub meta: KyokuMeta,
    pub scoreboard: Vec<i32>,
    pub dora_indicators: Vec<Tile>,
    pub ura_indicators: Vec<Tile>,
    pub action_tables: Vec<ActionTable>,
    pub end_status: EndStatus,
}

#[derive(Debug, Clone)]
pub enum EndStatus {
    Hora { details: Vec<HoraDetail> },
    Ryukyoku { score_deltas: Vec<i32> },
}

#[derive(Debug, Clone, Default)]
pub struct HoraDetail {
    pub who: u8,
    pub target: u8,
    pub score_deltas: Vec<i32>,
    pub yaku: Vec<String>,
}

/// A group of "配牌", "取" and "出", describing a player's
/// gaming status and actions throughout a kyoku.
#[derive(Debug, Clone)]
pub struct ActionTable {
    pub haipai: Vec<Tile>,
    pub takes: Vec<ActionItem>,
    pub discards: Vec<ActionItem>,
}

impl Log {
    /// Parse a tenhou.net/6 log from JSON string.
    #[inline]
    pub fn from_json_str(json_string: &str) -> Result<Self, ParseError> {
        let raw_log: RawLog = json::from_str(json_string)?;
        Self::try_from(raw_log)
    }

    #[inline]
    pub fn filter_kyokus(&mut self, kyoku_filter: &KyokuFilter) {
        self.kyokus
            .retain(|l| kyoku_filter.test(l.meta.kyoku_num, l.meta.honba));
    }
}

impl TryFrom<RawLog> for Log {
    type Error = ParseError;

    fn try_from(raw_log: RawLog) -> Result<Self, Self::Error> {
        let RawLog {
            logs, names, rule, ..
        } = raw_log;

        let num_players = if rule.disp.contains('三') || rule.disp.contains("3-Player") {
            3
        } else {
            4
        };
        let game_length = if rule.disp.contains('東') || rule.disp.contains("East") {
            GameLength::Tonpuu
        } else {
            GameLength::Hanchan
        };
        let has_aka = rule.aka + rule.aka51 + rule.aka52 + rule.aka53 > 0;

        let mut kyokus = Vec::with_capacity(logs.len());
        for log in logs {
            let action_tables = vec![
                ActionTable {
                    haipai: log.haipai_0.to_vec(),
                    takes: log.takes_0,
                    discards: log.discards_0,
                },
                ActionTable {
                    haipai: log.haipai_1.to_vec(),
                    takes: log.takes_1,
                    discards: log.discards_1,
                },
                ActionTable {
                    haipai: log.haipai_2.to_vec(),
                    takes: log.takes_2,
                    discards: log.discards_2,
                },
                ActionTable {
                    haipai: log.haipai_3,
                    takes: log.takes_3,
                    discards: log.discards_3,
                },
            ];
            if action_tables.len() < num_players {
                return Err(ParseError::UnsupportedPlayerCount(num_players));
            }

            let mut kyoku = Kyoku {
                meta: log.meta,
                scoreboard: log.scoreboard[..num_players].to_vec(),
                dora_indicators: log.dora_indicators,
                ura_indicators: log.ura_indicators,
                action_tables: action_tables.into_iter().take(num_players).collect(),
                end_status: EndStatus::Ryukyoku {
                    score_deltas: vec![0; num_players], // default
                },
            };

            if let Some(ResultItem::Status(status_text)) = log.results.first() {
                if status_text == "和了" {
                    let mut details = vec![];
                    for detail_tuple in log.results[1..].chunks_exact(2) {
                        if let [
                            ResultItem::ScoreDeltas(score_deltas),
                            ResultItem::HoraDetail(who_target_tuple),
                        ] = detail_tuple
                        {
                            let who = if let Some(Value::Number(n)) = who_target_tuple.first() {
                                n.as_u64().unwrap_or(0) as u8
                            } else {
                                return Err(ParseError::InvalidHoraDetail);
                            };
                            let target = if let Some(Value::Number(n)) = who_target_tuple.get(1) {
                                n.as_u64().unwrap_or(0) as u8
                            } else {
                                return Err(ParseError::InvalidHoraDetail);
                            };
                            let hora_detail = HoraDetail {
                                score_deltas: score_deltas[..num_players].to_vec(),
                                who,
                                target,
                                yaku: who_target_tuple
                                    .iter()
                                    .skip(4)
                                    .filter_map(|value| match value {
                                        Value::String(s) => Some(s.clone()),
                                        _ => None,
                                    })
                                    .collect(),
                            };
                            details.push(hora_detail);
                        }
                    }
                    kyoku.end_status = EndStatus::Hora { details };
                } else {
                    let score_deltas =
                        if let Some(ResultItem::ScoreDeltas(dts)) = log.results.get(1) {
                            dts[..num_players].to_vec()
                        } else {
                            vec![0; num_players]
                        };
                    kyoku.end_status = EndStatus::Ryukyoku { score_deltas };
                }
            }

            kyokus.push(kyoku);
        }

        Ok(Self {
            names: names[..num_players].to_vec(),
            num_players,
            game_length,
            has_aka,
            kyokus,
        })
    }
}
