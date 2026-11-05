use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Sequence,
    Loop,
    RepeatOne,
    Shuffle,
}
impl Mode {
    pub fn next(self) -> Self {
        match self {
            Self::Sequence => Self::Loop,
            Self::Loop => Self::RepeatOne,
            Self::RepeatOne => Self::Shuffle,
            Self::Shuffle => Self::Sequence,
        }
    }
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::Sequence, false) => "顺序播放",
            (Self::Loop, false) => "列表循环",
            (Self::RepeatOne, false) => "单曲循环",
            (Self::Shuffle, false) => "随机播放",
            (Self::Sequence, true) => "Sequence",
            (Self::Loop, true) => "Loop all",
            (Self::RepeatOne, true) => "Repeat one",
            (Self::Shuffle, true) => "Shuffle",
        }
    }
    pub fn target(
        self,
        index: u64,
        total: u64,
        forward: bool,
        automatic: bool,
        random: u64,
    ) -> Option<u64> {
        if total == 0 {
            return None;
        }
        if self == Self::RepeatOne && automatic {
            return Some(index.min(total - 1));
        }
        if self == Self::Shuffle {
            if total == 1 {
                return Some(0);
            }
            let mut next = random % (total - 1);
            if next >= index {
                next += 1;
            }
            return Some(next);
        }
        if forward {
            if index + 1 < total {
                Some(index + 1)
            } else if matches!(self, Self::Loop | Self::RepeatOne) {
                Some(0)
            } else {
                None
            }
        } else if index > 0 {
            Some(index - 1)
        } else if matches!(self, Self::Loop | Self::RepeatOne) {
            Some(total - 1)
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_handle_boundaries_and_repeat_without_immediate_shuffle_duplicates() {
        assert_eq!(Mode::Sequence.target(4, 5, true, true, 0), None);
        assert_eq!(Mode::Loop.target(4, 5, true, true, 0), Some(0));
        assert_eq!(Mode::RepeatOne.target(2, 5, true, true, 0), Some(2));
        assert_eq!(Mode::RepeatOne.target(2, 5, true, false, 0), Some(3));
        for r in 0..30 {
            assert_ne!(Mode::Shuffle.target(2, 5, true, true, r), Some(2));
        }
    }
}
