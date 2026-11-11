//! Undo history as a tree (an undo graph).
//!
//! Every state the document goes through is a node, and the edge from a node
//! to its parent is one reversible change: where it happened, the text it
//! removed and the text it inserted. Undo walks to the parent and redo to the
//! child visited last; editing after an undo starts a new branch instead of
//! throwing the undone steps away. Only the changes are stored, never whole
//! documents: any state is rebuilt by walking the changes between two nodes.

use std::time::{Instant, SystemTime};

/// "No node" for parent and child links.
pub(crate) const NONE: u32 = u32::MAX;
const MAX_NODES: usize = 10_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Where the history starts: the text as opened, or the oldest state kept.
    Start,
    Edit,
    /// Keyboard typing, grouped by word.
    Typing,
    /// The file changed on disk and was reloaded.
    External,
    /// Lines brought back from another state of the history.
    BroughtBack,
}

pub(crate) struct Node {
    /// Stable id in creation order; never reused.
    pub(crate) seq: u64,
    pub(crate) parent: u32,
    /// The child redo follows: the one created or visited last.
    pub(crate) last_child: u32,
    pub(crate) children: u32,
    pub(crate) depth: u32,
    pub(crate) start: usize,
    /// The parent's text at `start..start + removed.len()`.
    pub(crate) removed: String,
    /// This state's text at `start..start + inserted.len()`.
    pub(crate) inserted: String,
    /// Cursor and selection to restore when this change is undone.
    pub(crate) cursor_before: usize,
    pub(crate) selection_before: Option<(usize, usize)>,
    /// Cursor and selection when this state was left by an undo; redo
    /// restores them.
    pub(crate) after: Option<(usize, Option<(usize, usize)>)>,
    pub(crate) kind: Kind,
    /// Last time this change grew (typing groups by pauses).
    pub(crate) recorded_at: Instant,
    /// Wall clock of the change, for "5 min ago".
    pub(crate) at: SystemTime,
}

impl Node {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

pub(crate) struct History {
    /// In creation order (`seq` ascending); a parent always comes before its
    /// children.
    pub(crate) nodes: Vec<Node>,
    pub(crate) current: u32,
    saved: Option<u64>,
    next_seq: u64,
    bytes: usize,
    max_nodes: usize,
    max_bytes: usize,
    /// Changes whenever nodes or their changes do (not when only `current`
    /// moves): caches of the graph key on it.
    pub(crate) version: u64,
}

impl History {
    /// A history whose start is `saved` (the text matches the file) or not.
    pub(crate) fn new(saved: bool) -> Self {
        Self::with_limits(saved, MAX_NODES, MAX_BYTES)
    }

    pub(crate) fn with_limits(saved: bool, max_nodes: usize, max_bytes: usize) -> Self {
        Self {
            nodes: vec![Node {
                seq: 0,
                parent: NONE,
                last_child: NONE,
                children: 0,
                depth: 0,
                start: 0,
                removed: String::new(),
                inserted: String::new(),
                cursor_before: 0,
                selection_before: None,
                after: None,
                kind: Kind::Start,
                recorded_at: Instant::now(),
                at: SystemTime::now(),
            }],
            current: 0,
            saved: saved.then_some(0),
            next_seq: 1,
            bytes: 0,
            max_nodes: max_nodes.max(2),
            max_bytes,
            version: 0,
        }
    }

    /// Starts over from the text as it is now.
    pub(crate) fn reset(&mut self, saved: bool) {
        let version = self.version.wrapping_add(1);
        let next_seq = self.next_seq;
        *self = Self::with_limits(saved, self.max_nodes, self.max_bytes);
        // Seqs stay unique across resets: old ids never point at new nodes.
        self.nodes[0].seq = next_seq;
        self.next_seq = next_seq + 1;
        self.saved = saved.then_some(next_seq);
        self.version = version;
    }

    pub(crate) fn node(&self, index: u32) -> &Node {
        &self.nodes[index as usize]
    }

    pub(crate) fn current_node(&self) -> &Node {
        self.node(self.current)
    }

    pub(crate) fn current_seq(&self) -> u64 {
        self.current_node().seq
    }

    pub(crate) fn saved_seq(&self) -> Option<u64> {
        self.saved
    }

    /// The text now is the text last saved (or opened).
    pub(crate) fn at_saved(&self) -> bool {
        self.saved == Some(self.current_seq())
    }

    pub(crate) fn mark_saved(&mut self) {
        self.saved = Some(self.current_seq());
    }

    /// A save that started at state `seq` landed (if that state still exists).
    pub(crate) fn mark_saved_seq(&mut self, seq: u64) {
        self.saved = self.index_of(seq).map(|_| seq);
    }

    pub(crate) fn index_of(&self, seq: u64) -> Option<u32> {
        self.nodes
            .binary_search_by_key(&seq, |node| node.seq)
            .ok()
            .map(|index| index as u32)
    }

    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Records a change made on top of the current state; it becomes current.
    pub(crate) fn push(
        &mut self,
        start: usize,
        removed: String,
        inserted: String,
        cursor: usize,
        selection: Option<(usize, usize)>,
        kind: Kind,
    ) {
        let parent = self.current;
        let node = Node {
            seq: self.next_seq,
            parent,
            last_child: NONE,
            children: 0,
            depth: self.node(parent).depth + 1,
            start,
            removed,
            inserted,
            cursor_before: cursor,
            selection_before: selection,
            after: None,
            kind,
            recorded_at: Instant::now(),
            at: SystemTime::now(),
        };
        self.next_seq += 1;
        self.bytes += node.bytes();
        self.nodes.push(node);
        let index = (self.nodes.len() - 1) as u32;
        let parent = &mut self.nodes[parent as usize];
        parent.children += 1;
        parent.last_child = index;
        self.current = index;
        self.version = self.version.wrapping_add(1);
        self.prune_if_needed();
    }

    /// Typing grows the current change instead of adding another.
    pub(crate) fn extend_current(&mut self, text: &str) {
        let node = &mut self.nodes[self.current as usize];
        node.inserted.push_str(text);
        node.recorded_at = Instant::now();
        node.at = SystemTime::now();
        self.bytes += text.len();
        self.version = self.version.wrapping_add(1);
    }

    pub(crate) fn set_current_kind(&mut self, kind: Kind) {
        self.nodes[self.current as usize].kind = kind;
    }

    /// The changes to undo (in order) and then redo (in order) to go from
    /// state `from` to state `to`.
    pub(crate) fn path(&self, from: u32, to: u32) -> (Vec<u32>, Vec<u32>) {
        let (mut a, mut b) = (from, to);
        let (mut ups, mut downs) = (Vec::new(), Vec::new());
        while self.node(a).depth > self.node(b).depth {
            ups.push(a);
            a = self.node(a).parent;
        }
        while self.node(b).depth > self.node(a).depth {
            downs.push(b);
            b = self.node(b).parent;
        }
        while a != b {
            ups.push(a);
            a = self.node(a).parent;
            downs.push(b);
            b = self.node(b).parent;
        }
        downs.reverse();
        (ups, downs)
    }

    /// Applies a path from [`Self::path`] to `text`.
    pub(crate) fn rewind(&self, text: &mut GapText, ups: &[u32], downs: &[u32]) {
        for &up in ups {
            let node = self.node(up);
            text.splice(node.start, node.start + node.inserted.len(), &node.removed);
        }
        for &down in downs {
            let node = self.node(down);
            text.splice(node.start, node.start + node.removed.len(), &node.inserted);
        }
    }

    /// The text of state `target`, from the text of the current state.
    pub(crate) fn text_of(&self, current_text: &str, target: u32) -> String {
        let (ups, downs) = self.path(self.current, target);
        if ups.is_empty() && downs.is_empty() {
            return current_text.to_string();
        }
        let mut text = GapText::new(current_text);
        self.rewind(&mut text, &ups, &downs);
        text.to_string()
    }

    /// Moves a [`Walker`] to state `target` (rebuilding it from the current
    /// text when the history changed under it).
    pub(crate) fn walk(&self, walker: &mut Walker, current_text: &str, target: u32) {
        let from = match self.index_of(walker.at) {
            Some(from) if walker.version == self.version => from,
            _ => {
                *walker = Walker {
                    at: self.current_seq(),
                    version: self.version,
                    text: GapText::new(current_text),
                };
                self.current
            }
        };
        let (ups, downs) = self.path(from, target);
        self.rewind(&mut walker.text, &ups, &downs);
        walker.at = self.node(target).seq;
    }

    /// Drops the oldest history once it is too long or too heavy: first the
    /// branches nobody is on, oldest first; the path to the current state
    /// only loses its oldest steps, and the last change is always kept.
    fn prune_if_needed(&mut self) {
        if self.nodes.len() <= self.max_nodes && self.bytes <= self.max_bytes {
            return;
        }
        let target_nodes = (self.max_nodes - self.max_nodes / 10).max(2);
        let target_bytes = self.max_bytes - self.max_bytes / 10;
        let n = self.nodes.len();
        let mut child_lists: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (index, node) in self.nodes.iter().enumerate() {
            if node.parent != NONE {
                child_lists[node.parent as usize].push(index as u32);
            }
        }
        let mut on_path = vec![false; n];
        let mut walk = self.current;
        while walk != NONE {
            on_path[walk as usize] = true;
            walk = self.node(walk).parent;
        }
        let mut alive = vec![true; n];
        let mut children: Vec<u32> = self.nodes.iter().map(|node| node.children).collect();
        let mut leaves = std::collections::BinaryHeap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if node.children == 0 && !on_path[index] {
                leaves.push(std::cmp::Reverse((node.seq, index as u32)));
            }
        }
        let mut root = 0u32;
        let mut count = n;
        let mut bytes = self.bytes;
        let mut cleared_root_child = None;
        while count > target_nodes || bytes > target_bytes {
            let leaf = leaves.peek().map(|std::cmp::Reverse(entry)| *entry);
            let merge = (children[root as usize] == 1 && root != self.current)
                .then(|| {
                    child_lists[root as usize]
                        .iter()
                        .copied()
                        .find(|child| alive[*child as usize])
                })
                .flatten()
                .filter(|child| *child != self.current);
            let merge_first = match (leaf, merge) {
                (Some((leaf_seq, _)), Some(child)) => self.node(child).seq < leaf_seq,
                (None, Some(_)) => true,
                (Some(_), None) => false,
                (None, None) => break,
            };
            if merge_first {
                // The oldest change on the path folds into the start.
                let child = merge.expect("merge candidate");
                alive[root as usize] = false;
                count -= 1;
                bytes -= self.node(child).bytes();
                cleared_root_child = Some(child);
                root = child;
            } else {
                let std::cmp::Reverse((_, index)) = leaves.pop().expect("leaf");
                alive[index as usize] = false;
                count -= 1;
                bytes -= self.node(index).bytes();
                let parent = self.node(index).parent;
                children[parent as usize] -= 1;
                if children[parent as usize] == 0 && !on_path[parent as usize] {
                    leaves.push(std::cmp::Reverse((self.node(parent).seq, parent)));
                }
            }
        }
        // Compact: alive nodes keep their order, links are renumbered.
        let mut remap = vec![NONE; n];
        let mut next = 0u32;
        for index in 0..n {
            if alive[index] {
                remap[index] = next;
                next += 1;
            }
        }
        let last_children: Vec<u32> = (0..n)
            .map(|index| {
                let last = self.nodes[index].last_child;
                if last != NONE && alive[last as usize] {
                    remap[last as usize]
                } else {
                    child_lists[index]
                        .iter()
                        .rev()
                        .find(|child| alive[**child as usize])
                        .map(|child| remap[*child as usize])
                        .unwrap_or(NONE)
                }
            })
            .collect();
        let old = std::mem::take(&mut self.nodes);
        let mut nodes = Vec::with_capacity(count);
        for (index, mut node) in old.into_iter().enumerate() {
            if !alive[index] {
                continue;
            }
            node.last_child = last_children[index];
            node.children = 0;
            if node.parent == NONE || !alive[node.parent as usize] {
                node.parent = NONE;
                node.depth = 0;
            } else {
                node.parent = remap[node.parent as usize];
            }
            if cleared_root_child == Some(index as u32) {
                node.removed = String::new();
                node.inserted = String::new();
                node.kind = Kind::Start;
                node.parent = NONE;
            }
            nodes.push(node);
        }
        for index in 0..nodes.len() {
            let parent = nodes[index].parent;
            if parent != NONE {
                nodes[index].depth = nodes[parent as usize].depth + 1;
                nodes[parent as usize].children += 1;
            }
        }
        self.nodes = nodes;
        self.current = remap[self.current as usize];
        self.bytes = bytes;
        if let Some(saved) = self.saved {
            self.saved = self.index_of(saved).map(|_| saved);
        }
        self.version = self.version.wrapping_add(1);
    }
}

/// A copy of some state's text that moves along the history one change at a
/// time (the undo view browses with it).
pub(crate) struct Walker {
    at: u64,
    version: u64,
    text: GapText,
}

impl Walker {
    pub(crate) fn new() -> Self {
        Self {
            at: u64::MAX,
            version: u64::MAX,
            text: GapText::new(""),
        }
    }

    pub(crate) fn text(&self) -> String {
        self.text.to_string()
    }
}

/// Text with a gap at the last edit: a run of nearby changes only moves the
/// bytes between them instead of the whole text.
pub(crate) struct GapText {
    buf: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
}

impl GapText {
    pub(crate) fn new(text: &str) -> Self {
        let gap = 4096;
        let mut buf = Vec::with_capacity(text.len() + gap);
        buf.extend_from_slice(text.as_bytes());
        let gap_start = buf.len();
        buf.resize(text.len() + gap, 0);
        let gap_end = buf.len();
        Self {
            buf,
            gap_start,
            gap_end,
        }
    }

    fn move_gap(&mut self, to: usize) {
        if to < self.gap_start {
            let moved = self.gap_start - to;
            self.buf
                .copy_within(to..self.gap_start, self.gap_end - moved);
            self.gap_start = to;
            self.gap_end -= moved;
        } else if to > self.gap_start {
            let moved = to - self.gap_start;
            self.buf
                .copy_within(self.gap_end..self.gap_end + moved, self.gap_start);
            self.gap_start += moved;
            self.gap_end += moved;
        }
    }

    pub(crate) fn splice(&mut self, start: usize, end: usize, text: &str) {
        self.move_gap(start);
        self.gap_end += end - start;
        if self.gap_end - self.gap_start < text.len() {
            let extra = text.len() + self.buf.len() / 2 + 4096;
            let old_len = self.buf.len();
            self.buf.resize(old_len + extra, 0);
            self.buf
                .copy_within(self.gap_end..old_len, self.gap_end + extra);
            self.gap_end += extra;
        }
        self.buf[self.gap_start..self.gap_start + text.len()].copy_from_slice(text.as_bytes());
        self.gap_start += text.len();
    }

    pub(crate) fn to_string(&self) -> String {
        let mut bytes = Vec::with_capacity(self.buf.len() - (self.gap_end - self.gap_start));
        bytes.extend_from_slice(&self.buf[..self.gap_start]);
        bytes.extend_from_slice(&self.buf[self.gap_end..]);
        String::from_utf8(bytes)
            .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_text_matches_plain_string_splices() {
        let mut plain = String::from("hello world, café 雪");
        let mut gap = GapText::new(&plain);
        let mut seed = 7_u64;
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..2000 {
            let bounds: Vec<usize> = plain
                .char_indices()
                .map(|(i, _)| i)
                .chain([plain.len()])
                .collect();
            let a = bounds[random() % bounds.len()];
            let b = bounds[random() % bounds.len()];
            let (start, end) = (a.min(b), a.max(b));
            let long = "z".repeat(5000);
            let insert = ["", "x", "雪λ", "\n", "a longer run of text\n", &long][random() % 6];
            if plain.len() > 20_000 && !insert.is_empty() {
                continue;
            }
            plain.replace_range(start..end, insert);
            gap.splice(start, end, insert);
        }
        assert_eq!(gap.to_string(), plain);
    }

    #[test]
    fn path_goes_up_to_the_common_state_and_down_the_other_branch() {
        let mut history = History::new(true);
        history.push(0, String::new(), "a".into(), 0, None, Kind::Edit); // 1
        history.push(1, String::new(), "b".into(), 1, None, Kind::Edit); // 2
        history.current = 1;
        history.push(1, String::new(), "c".into(), 1, None, Kind::Edit); // 3
        let (ups, downs) = history.path(2, 3);
        assert_eq!(ups, vec![2]);
        assert_eq!(downs, vec![3]);
        assert_eq!(history.text_of("ac", 2), "ab");
        assert_eq!(history.text_of("ac", 0), "");
        assert_eq!(history.node(1).last_child, 3, "redo follows the newest branch");
    }
}
