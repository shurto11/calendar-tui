use crate::meta::StackCategory;
use ratatui::layout::{Constraint, Direction as LayoutDirection, Layout, Margin, Rect};
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

/// グラフ上のタスクラベルの最大表示幅(セル)
pub const MAX_LABEL_WIDTH: usize = 16;
/// 軸の端(矢印・目盛)とラベルが衝突しないための余白
const MARGIN_X: u16 = 2;
const MARGIN_Y: u16 = 1;

/// レイアウト計算への入力(1タスク分)
pub struct MatrixItem {
    pub task_id: String,
    pub title: String,
    pub imp: u8,  // 0-10
    pub clau: u8, // 0-10
    pub pri: Option<u32>,
    pub stack: Option<StackCategory>,
}

/// 配置決定済みのタスクラベル
#[derive(Debug, Clone)]
pub struct PlacedTask {
    pub task_id: String,
    pub label: String,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// サイドバーの幅(セル)
pub const SIDEBAR_WIDTH: u16 = 36;

/// マトリックス画面を (グラフ領域, サイドバー領域) に分割する。
/// app側のナビゲーションと ui側の描画で同一レイアウトを共有するための関数。
pub fn split_layout(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(LayoutDirection::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(SIDEBAR_WIDTH)])
        .split(area);
    (chunks[0], chunks[1])
}

/// 画面全体からグラフの内側(枠線を除いた描画領域)を求める
pub fn graph_inner(viewport: Rect) -> Rect {
    split_layout(viewport).0.inner(Margin::new(1, 1))
}

pub fn truncate_to_width(s: &str, max_cols: usize) -> String {
    if s.width() <= max_cols {
        return s.to_string();
    }
    let target = max_cols.saturating_sub(1);
    let mut used = 0;
    let mut end = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if used + cw > target {
            break;
        }
        used += cw;
        end += c.len_utf8();
    }
    format!("{}…", &s[..end])
}

/// 0-10 の値を area 内の座標へ写像する(交点 = 中心、5.0 が原点)
fn ideal_position(area: Rect, imp: u8, clau: u8) -> (i32, i32) {
    let cx = area.x as i32 + area.width as i32 / 2;
    let cy = area.y as i32 + area.height as i32 / 2;
    let usable_w = area.width.saturating_sub(2 * MARGIN_X) as f64;
    let usable_h = area.height.saturating_sub(2 * MARGIN_Y) as f64;
    let dx = (clau as f64 - 5.0) / 10.0 * usable_w;
    let dy = (imp as f64 - 5.0) / 10.0 * usable_h;
    (cx + dx.round() as i32, cy - dy.round() as i32)
}

/// `ideal_position` の逆変換。area 内のセル座標 (x,y) を imp/clau(0-10)へ戻す。
/// タッチでタスクを再配置するときに使う。
pub fn coords_to_imp_clau(area: Rect, x: u16, y: u16) -> (u8, u8) {
    let cx = area.x as f64 + area.width as f64 / 2.0;
    let cy = area.y as f64 + area.height as f64 / 2.0;
    let usable_w = area.width.saturating_sub(2 * MARGIN_X) as f64;
    let usable_h = area.height.saturating_sub(2 * MARGIN_Y) as f64;
    let clau = if usable_w > 0.0 {
        ((x as f64 - cx) / usable_w * 10.0 + 5.0).round()
    } else {
        5.0
    };
    // 画面 y は下ほど大きいので imp は上下反転(cy - y)。
    let imp = if usable_h > 0.0 {
        ((cy - y as f64) / usable_h * 10.0 + 5.0).round()
    } else {
        5.0
    };
    (imp.clamp(0.0, 10.0) as u8, clau.clamp(0.0, 10.0) as u8)
}

/// 全タスクの表示位置を決める。優先順位昇順(None最後)に理想位置へ置き、
/// 重なる場合は上下に1行ずつずらして空きを探す。
pub fn compute_layout(items: &[MatrixItem], area: Rect) -> Vec<PlacedTask> {
    if area.width < 2 * MARGIN_X + 4 || area.height < 2 * MARGIN_Y + 2 {
        return Vec::new();
    }

    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| (items[i].pri.unwrap_or(u32::MAX), i));

    // 行ごとの占有x区間 (start, end_exclusive)
    let mut occupied: HashMap<u16, Vec<(u16, u16)>> = HashMap::new();
    let mut placed = Vec::with_capacity(items.len());

    let top = area.y as i32;
    let bottom = area.y as i32 + area.height as i32 - 1;

    for &i in &order {
        let item = &items[i];
        // スタック済みは種類マーク、それ以外は優先順位番号(無ければ"-")
        let prefix = match item.stack {
            Some(cat) => cat.mark().to_string(),
            None => item
                .pri
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string()),
        };
        let label = truncate_to_width(&format!("{} {}", prefix, item.title), MAX_LABEL_WIDTH);
        let width = label.width() as u16;

        let (ix, iy) = ideal_position(area, item.imp, item.clau);
        // ラベルが領域からはみ出さないよう x を clamp
        let min_x = area.x as i32;
        let max_x = (area.x + area.width).saturating_sub(width) as i32;
        let x = ix.clamp(min_x, max_x.max(min_x)) as u16;

        // dy = 0, +1, -1, +2, -2, ... の順で空き行を探す
        let mut chosen: Option<u16> = None;
        let max_span = area.height as i32;
        'search: for step in 0..max_span {
            for dy in [step, -step] {
                let y = iy + dy;
                if y < top || y > bottom {
                    continue;
                }
                let y = y as u16;
                let rows = occupied.entry(y).or_default();
                // 前後1セルの隙間込みで重なり判定
                let overlaps = rows.iter().any(|&(s, e)| {
                    let new_s = x.saturating_sub(1);
                    let new_e = x + width + 1;
                    new_s < e && s < new_e
                });
                if !overlaps {
                    rows.push((x, x + width));
                    chosen = Some(y);
                    break 'search;
                }
                if step == 0 {
                    break; // dy=0 は1回だけ
                }
            }
        }

        if let Some(y) = chosen {
            placed.push(PlacedTask {
                task_id: item.task_id.clone(),
                label,
                x,
                y,
            });
        }
        // 置き場が無い場合(画面が極端に狭い)は描画しない
    }

    placed
}

/// hjkl 方向の最近傍タスクを選ぶ。戻り値は移動先の task_id。
pub fn navigate<'a>(
    placed: &'a [PlacedTask],
    current_id: &str,
    dir: Direction,
) -> Option<&'a str> {
    let cur = placed.iter().find(|p| p.task_id == current_id)?;
    // ラベル幅の違いで同列タスクが左右候補に化けないよう、始点xで比較する
    let (cx, cy) = (cur.x as f64, cur.y as f64);

    let mut best: Option<(f64, &PlacedTask)> = None;
    for p in placed {
        if p.task_id == cur.task_id {
            continue;
        }
        let (px, py) = (p.x as f64, p.y as f64);
        // 文字セルは横長なので、縦移動と横移動でペナルティ係数を変える
        let score = match dir {
            Direction::Right if px > cx => (px - cx) + 2.5 * (py - cy).abs(),
            Direction::Left if px < cx => (cx - px) + 2.5 * (py - cy).abs(),
            Direction::Down if py > cy => (py - cy) + 0.4 * (px - cx).abs(),
            Direction::Up if py < cy => (cy - py) + 0.4 * (px - cx).abs(),
            _ => continue,
        };
        if best.map_or(true, |(s, _)| score < s) {
            best = Some((score, p));
        }
    }
    best.map(|(_, p)| p.task_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, imp: u8, clau: u8, pri: Option<u32>) -> MatrixItem {
        MatrixItem {
            task_id: id.to_string(),
            title: format!("task-{}", id),
            imp,
            clau,
            pri,
            stack: None,
        }
    }

    fn area() -> Rect {
        Rect::new(0, 0, 80, 24)
    }

    #[test]
    fn high_imp_is_upper_high_clau_is_right() {
        let items = vec![item("a", 10, 10, Some(1)), item("b", 1, 1, Some(2))];
        let placed = compute_layout(&items, area());
        let a = placed.iter().find(|p| p.task_id == "a").unwrap();
        let b = placed.iter().find(|p| p.task_id == "b").unwrap();
        assert!(a.y < b.y, "重要度が高いほど上");
        assert!(a.x > b.x, "clau度が高いほど右");
    }

    #[test]
    fn center_task_is_near_center() {
        // 5.0 が原点なので imp=5,clau=5 はちょうど中心
        let items = vec![item("c", 5, 5, None)];
        let placed = compute_layout(&items, area());
        let c = &placed[0];
        assert!((c.y as i32 - 12).abs() <= 2);
    }

    #[test]
    fn same_position_does_not_overlap() {
        let items = vec![
            item("a", 5, 5, Some(1)),
            item("b", 5, 5, Some(2)),
            item("c", 5, 5, Some(3)),
        ];
        let placed = compute_layout(&items, area());
        assert_eq!(placed.len(), 3);
        for i in 0..placed.len() {
            for j in (i + 1)..placed.len() {
                let (p, q) = (&placed[i], &placed[j]);
                let (pw, qw) = (p.label.width() as u16, q.label.width() as u16);
                let overlap = p.y == q.y && p.x < q.x + qw && q.x < p.x + pw;
                assert!(!overlap, "{} と {} が重なっている", p.task_id, q.task_id);
            }
        }
    }

    #[test]
    fn label_stays_inside_area() {
        let items = vec![item("r", 10, 10, Some(1)), item("l", 1, 1, Some(2))];
        let a = area();
        for p in compute_layout(&items, a) {
            let w = p.label.width() as u16;
            assert!(p.x >= a.x && p.x + w <= a.x + a.width);
            assert!(p.y >= a.y && p.y < a.y + a.height);
        }
    }

    #[test]
    fn navigate_picks_directional_neighbor() {
        let items = vec![
            item("center", 5, 5, Some(1)),
            item("right", 5, 9, Some(2)),
            item("up", 9, 5, Some(3)),
        ];
        let placed = compute_layout(&items, area());
        assert_eq!(navigate(&placed, "center", Direction::Right), Some("right"));
        assert_eq!(navigate(&placed, "center", Direction::Up), Some("up"));
        assert_eq!(navigate(&placed, "center", Direction::Left), None);
    }

    #[test]
    fn coords_to_imp_clau_roundtrips() {
        let a = area();
        for (imp, clau) in [(5u8, 5u8), (10, 10), (0, 0), (8, 2), (3, 7)] {
            let (x, y) = ideal_position(a, imp, clau);
            let (ri, rc) = coords_to_imp_clau(a, x as u16, y as u16);
            assert!((ri as i32 - imp as i32).abs() <= 1, "imp {imp} -> {ri}");
            assert!((rc as i32 - clau as i32).abs() <= 1, "clau {clau} -> {rc}");
        }
    }

    #[test]
    fn japanese_label_truncated_on_char_boundary() {
        let s = truncate_to_width("とても長い日本語のタスク名です", MAX_LABEL_WIDTH);
        assert!(s.width() <= MAX_LABEL_WIDTH);
        assert!(s.ends_with('…'));
    }
}
