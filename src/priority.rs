use chrono::NaiveDate;

/// 1タスク分のスコア計算入力
pub struct ScoreInput {
    pub imp: u8,  // 重要度 0-10
    pub clau: u8, // clau度 0-10
    pub due: Option<NaiveDate>,
}

/// スコア = (11-imp)*(11-clau)*日数項。
/// 日数項 = max(0, due - today)。期限切れ・当日は 0、期限なしは 7(一週間後扱い)。
/// 値が低いものほど優先順位が高い。
pub fn score(input: &ScoreInput, today: NaiveDate) -> i64 {
    let days = match input.due {
        Some(d) => (d - today).num_days().max(0),
        None => 7,
    };
    (11 - input.imp as i64) * (11 - input.clau as i64) * days
}

/// スコア昇順で競争順位を割り当てる。
/// 1始まり、同スコアは同順位、その分だけ次の順位をスキップ(例: 1,1,3)。
/// 戻り値は入力と同じ並びの順位ベクタ。
pub fn rank(scores: &[i64]) -> Vec<u32> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by_key(|&i| scores[i]);

    let mut ranks = vec![0u32; scores.len()];
    let mut last_score: Option<i64> = None;
    let mut last_rank = 0u32;
    for (pos, &i) in idx.iter().enumerate() {
        let r = if last_score == Some(scores[i]) {
            last_rank
        } else {
            pos as u32 + 1
        };
        ranks[i] = r;
        last_score = Some(scores[i]);
        last_rank = r;
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 16).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn score_uses_days_until_due() {
        // 7日後、imp=5, clau=5 → (11-5)*(11-5)*7 = 252
        let s = score(
            &ScoreInput { imp: 5, clau: 5, due: Some(d("2026-06-23")) },
            today(),
        );
        assert_eq!(s, 6 * 6 * 7);
    }

    #[test]
    fn no_due_treated_as_seven_days() {
        let with_none = score(&ScoreInput { imp: 3, clau: 8, due: None }, today());
        let with_week = score(
            &ScoreInput { imp: 3, clau: 8, due: Some(d("2026-06-23")) },
            today(),
        );
        assert_eq!(with_none, with_week);
    }

    #[test]
    fn overdue_and_today_clamp_to_zero() {
        let overdue = score(
            &ScoreInput { imp: 0, clau: 0, due: Some(d("2026-06-01")) },
            today(),
        );
        let due_today = score(
            &ScoreInput { imp: 9, clau: 2, due: Some(d("2026-06-16")) },
            today(),
        );
        assert_eq!(overdue, 0);
        assert_eq!(due_today, 0);
    }

    #[test]
    fn lower_score_first_and_ties_skip_next_rank() {
        // scores: [5, 1, 1, 9] → sorted 1,1,5,9 → ranks 1,1,3,4
        let ranks = rank(&[5, 1, 1, 9]);
        assert_eq!(ranks, vec![3, 1, 1, 4]);
    }

    #[test]
    fn empty_is_empty() {
        assert!(rank(&[]).is_empty());
    }
}
