//! The order, the placement and the scene text of the labels of a frame.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use galaxy_scene::ObjectId;

/// Pixels of the em of a label at the scale factor 1.
pub const EM: f32 = 11.0;
/// Height in pixels of the line of a label at the scale factor 1.
pub const LINE: f32 = 16.0;
/// Width in pixels of the halo of a label at the scale factor 1.
pub const HALO: f32 = 2.0;
/// Distance in pixels of a label from its anchor at the scale factor 1.
pub const OFFSET: f32 = 4.0;
/// Size on the screen in pixels from which a node with no child and a closed node have a label,
/// at the scale factor 1.
pub const SHOW: f32 = 48.0;
/// Side in pixels of a cell of the overlap grid at the scale factor 1.
pub const CELL: f32 = 64.0;
/// Largest count of the candidates of a frame.
pub const CANDIDATES: usize = 4096;
/// Largest count of the glyphs of the labels of a frame.
pub const GLYPHS: usize = 65_536;
/// Largest count of the hidden lines of the scene text.
pub const HIDDEN_LINES: usize = 256;

/// A node with a label in a frame and its size on the screen in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub node: ObjectId,
    pub size: f32,
}

/// The indexes of `candidates` in priority order: the node `selected`, then the nodes of `path`
/// in path order, then the other candidates by size, the larger first, and on equal sizes by the
/// node id in byte order.
pub fn order(
    candidates: &[Candidate],
    selected: Option<&ObjectId>,
    path: &[ObjectId],
) -> Vec<usize> {
    let mut steps: BTreeMap<&ObjectId, usize> = BTreeMap::new();
    for (step, node) in path.iter().enumerate() {
        steps.entry(node).or_insert(step);
    }
    let tier = |candidate: &Candidate| {
        if Some(&candidate.node) == selected {
            (0, 0)
        } else if let Some(&step) = steps.get(&candidate.node) {
            (1, step)
        } else {
            (2, 0)
        }
    };
    let mut indexes: Vec<usize> = (0..candidates.len()).collect();
    indexes.sort_by(|&first, &second| {
        let [a, b] = [&candidates[first], &candidates[second]];
        tier(a)
            .cmp(&tier(b))
            .then(b.size.total_cmp(&a.size))
            .then(a.node.as_str().as_bytes().cmp(b.node.as_str().as_bytes()))
    });
    indexes
}

/// `true` when the rectangles `a` and `b`, as left, top, right and bottom, have a common area.
pub fn overlap(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// The rectangles of the shown labels of a frame, with the shown labels that touch each cell of
/// a grid over the frame.
pub struct Grid {
    cell: f32,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<usize>>,
    rects: Vec<[f32; 4]>,
}

impl Grid {
    /// The empty grid of a frame of `width` by `height` pixels with cells of `cell` pixels.
    pub fn new(width: f32, height: f32, cell: f32) -> Self {
        let cell = cell.max(1.0);
        let columns = ((width / cell).ceil() as usize).max(1);
        let rows = ((height / cell).ceil() as usize).max(1);
        Self {
            cell,
            columns,
            rows,
            cells: vec![Vec::new(); columns * rows],
            rects: Vec::new(),
        }
    }

    /// The count of the columns and of the rows of cells.
    pub fn dimensions(&self) -> [usize; 2] {
        [self.columns, self.rows]
    }

    /// The columns and the rows of the cells that `rect` touches.
    fn span(&self, rect: &[f32; 4]) -> [RangeInclusive<usize>; 2] {
        let index = |value: f32, count: usize| {
            ((value / self.cell).floor().max(0.0) as usize).min(count - 1)
        };
        [
            index(rect[0], self.columns)..=index(rect[2], self.columns),
            index(rect[1], self.rows)..=index(rect[3], self.rows),
        ]
    }

    /// The first shown label whose rectangle overlaps `rect`.
    pub fn hit(&self, rect: &[f32; 4]) -> Option<usize> {
        let [columns, rows] = self.span(rect);
        let mut first: Option<usize> = None;
        for row in rows {
            for column in columns.clone() {
                for &shown in &self.cells[row * self.columns + column] {
                    if first.is_none_or(|first| shown < first) && overlap(&self.rects[shown], rect)
                    {
                        first = Some(shown);
                    }
                }
            }
        }
        first
    }

    /// Adds `rect` as the next shown label and gives its index.
    pub fn insert(&mut self, rect: [f32; 4]) -> usize {
        let shown = self.rects.len();
        let [columns, rows] = self.span(&rect);
        for row in rows {
            for column in columns.clone() {
                self.cells[row * self.columns + column].push(shown);
            }
        }
        self.rects.push(rect);
        shown
    }
}

/// The place of each rectangle of `rects` in priority order in a frame of `width` by `height`
/// pixels with grid cells of `cell` pixels: `None` for a shown rectangle, and the index of the
/// first shown rectangle that it overlaps for a hidden rectangle.
pub fn place(rects: &[[f32; 4]], width: f32, height: f32, cell: f32) -> Vec<Option<usize>> {
    let mut grid = Grid::new(width, height, cell);
    let mut shown: Vec<usize> = Vec::new();
    rects
        .iter()
        .enumerate()
        .map(|(index, rect)| match grid.hit(rect) {
            Some(first) => Some(shown[first]),
            None => {
                grid.insert(*rect);
                shown.push(index);
                None
            }
        })
        .collect()
}

/// The cause of a hidden label: the shown label of the highest priority that it overlaps, no
/// room in the atlas for a glyph, or the limit of the candidates or of the glyphs of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    Overlap(ObjectId),
    Atlas,
    Limit,
}

/// A shown label: its node and its rectangle in pixels as left, top, right and bottom, with its
/// halo.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub node: ObjectId,
    pub rect: [f32; 4],
}

/// A hidden label: its node and its cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hidden {
    pub node: ObjectId,
    pub cause: Cause,
}

/// The shown and the hidden labels of a frame, each in priority order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Labels {
    pub shown: Vec<Shown>,
    pub hidden: Vec<Hidden>,
}

impl fmt::Display for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "label {} hidden ", self.node.as_str())?;
        match &self.cause {
            Cause::Overlap(node) => write!(f, "overlap={}", node.as_str()),
            Cause::Atlas => f.write_str("atlas"),
            Cause::Limit => f.write_str("limit"),
        }
    }
}

impl Labels {
    /// The lines of the scene text: the count line, 1 line for each of the first `HIDDEN_LINES`
    /// hidden labels, and the count of the other hidden labels when it is above 0.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "labels shown={} hidden={}",
            self.shown.len(),
            self.hidden.len()
        )];
        lines.extend(
            self.hidden
                .iter()
                .take(HIDDEN_LINES)
                .map(ToString::to_string),
        );
        let unlisted = self.hidden.len().saturating_sub(HIDDEN_LINES);
        if unlisted > 0 {
            lines.push(format!("labels unlisted={unlisted}"));
        }
        lines
    }
}
