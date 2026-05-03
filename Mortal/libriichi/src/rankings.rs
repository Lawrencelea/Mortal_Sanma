#[derive(Debug, Clone, Copy)]
pub struct Rankings {
    pub player_by_rank: [u8; 4],
    pub rank_by_player: [u8; 4],
}

impl Rankings {
    pub fn new(scores: [i32; 4]) -> Self {
        let mut player_by_rank = [0, 1, 2, 3];
        player_by_rank.sort_by_key(|&i| -scores[i as usize]);

        let mut rank_by_player = [0; 4];
        for (rank, id) in player_by_rank.into_iter().enumerate() {
            rank_by_player[id as usize] = rank as u8;
        }

        Self {
            player_by_rank,
            rank_by_player,
        }
    }

    /// Compute rankings for `n` players (n <= 4) using only the first n scores.
    pub fn new_n(scores: &[i32; 4], n: usize) -> Self {
        debug_assert!(n <= 4);
        let mut player_by_rank = [0u8, 1, 2, 3];
        player_by_rank[..n].sort_by_key(|&i| -scores[i as usize]);

        let mut rank_by_player = [0u8; 4];
        for (rank, &id) in player_by_rank[..n].iter().enumerate() {
            rank_by_player[id as usize] = rank as u8;
        }
        // Players beyond n get rank n (last+1, but unused).
        for i in n..4 {
            rank_by_player[i] = n as u8;
        }

        Self {
            player_by_rank,
            rank_by_player,
        }
    }

    pub fn new_sanma(scores: [i32; 3]) -> Self {
        let mut scores4 = [0; 4];
        scores4[..3].copy_from_slice(&scores);
        Self::new_n(&scores4, 3)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn rankings() {
        let mut scores = [25000, 25000, 30000, 20000];

        let rk = Rankings::new(scores);
        assert_eq!(rk.player_by_rank, [2, 0, 1, 3]);
        assert_eq!(rk.rank_by_player, [1, 2, 0, 3]);
        *scores.iter_mut().min_by_key(|s| -**s).unwrap() = 0;
        assert_eq!(scores, [25000, 25000, 0, 20000]);

        scores = [25000, 25000, 25000, 25000];
        let rk = Rankings::new(scores);
        assert_eq!(rk.player_by_rank, [0, 1, 2, 3]);
        assert_eq!(rk.rank_by_player, [0, 1, 2, 3]);
        *scores.iter_mut().min_by_key(|s| -**s).unwrap() = 0;
        assert_eq!(scores, [0, 25000, 25000, 25000]);

        scores = [18000, 32000, 32000, 18000];
        let rk = Rankings::new(scores);
        assert_eq!(rk.player_by_rank, [1, 2, 0, 3]);
        assert_eq!(rk.rank_by_player, [2, 0, 1, 3]);
        *scores.iter_mut().min_by_key(|s| -**s).unwrap() = 0;
        assert_eq!(scores, [18000, 0, 32000, 18000]);

        scores = [32000, 18000, 18000, 32000];
        let rk = Rankings::new(scores);
        assert_eq!(rk.player_by_rank, [0, 3, 1, 2]);
        assert_eq!(rk.rank_by_player, [0, 2, 3, 1]);
        *scores.iter_mut().min_by_key(|s| -**s).unwrap() = 0;
        assert_eq!(scores, [0, 18000, 18000, 32000]);

        scores = [0, 100000, 0, 0];
        let rk = Rankings::new(scores);
        assert_eq!(rk.player_by_rank, [1, 0, 2, 3]);
        assert_eq!(rk.rank_by_player, [1, 0, 2, 3]);
        *scores.iter_mut().min_by_key(|s| -**s).unwrap() = 0;
        assert_eq!(scores, [0; 4]);
    }
}
