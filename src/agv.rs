//! AGV fleet and lane network.
//!
//! The lane network is a directed graph of segments. Each segment holds
//! at most one AGV — this mutual exclusion is the source of potential
//! deadlocks. The module provides shortest-path routing and a
//! [`WaitForGraph`] used by the scheduler's deadlock detector.

use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::*;

// ── AGV ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize)]
pub struct Agv {
    pub id: AgvId,
    pub state: AgvState,
    pub segment: SegmentId,
    pub cargo: Cargo,
    pub path: Vec<SegmentId>,
    pub path_cursor: usize,
    // Metrics
    pub distance_traveled: u64, // segments
    pub loads_delivered: u64,
}

impl Agv {
    pub fn new(id: AgvId, start: SegmentId) -> Self {
        Self {
            id,
            state: AgvState::Idle,
            segment: start,
            cargo: Cargo::Empty,
            path: Vec::new(),
            path_cursor: 0,
            distance_traveled: 0,
            loads_delivered: 0,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == AgvState::Idle
    }

    /// The next segment this AGV wants to enter, if traveling.
    pub fn next_segment(&self) -> Option<SegmentId> {
        if self.path_cursor < self.path.len() {
            Some(self.path[self.path_cursor])
        } else {
            None
        }
    }

    pub fn advance(&mut self) {
        if self.path_cursor < self.path.len() {
            self.segment = self.path[self.path_cursor];
            self.path_cursor += 1;
            self.distance_traveled += 1;
        }
    }

    pub fn at_destination(&self) -> bool {
        self.path_cursor >= self.path.len()
    }

    pub fn fault(&mut self) {
        self.state = AgvState::Faulted;
    }

    pub fn repair(&mut self) {
        self.state = AgvState::Idle;
    }
}

// ── Lane network ────────────────────────────────────────────────────
/// Directed graph of lane segments with occupancy tracking.
#[derive(Debug, Clone)]
pub struct LaneNetwork {
    /// Adjacency list: segment → reachable neighbors.
    adj: Vec<Vec<SegmentId>>,
    /// Which AGV (if any) occupies each segment.
    occupant: Vec<Option<AgvId>>,
}

impl Default for LaneNetwork {
    fn default() -> Self {
        let mut adj = vec![Vec::new(); TOTAL_SEGMENTS];

        for i in 0..LOOP_SEGMENTS {
            let next = (i + 1) % LOOP_SEGMENTS;
            adj[i].push(next);
            adj[next].push(i);
        }

        for mid in 0..NUM_MILLS {
            let loop_seg = mill_loop_segment(mid);
            let spur = mill_spur(mid);
            adj[loop_seg].push(spur);
            adj[spur].push(loop_seg);
        }

        Self {
            adj,
            occupant: vec![None; TOTAL_SEGMENTS],
        }
    }
}

impl LaneNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shortest path from `src` to `dst` (BFS, unweighted).
    /// Returns the sequence of segments to traverse *excluding* `src`.
    pub fn route(&self, src: SegmentId, dst: SegmentId) -> Option<Vec<SegmentId>> {
        if src == dst {
            return Some(Vec::new());
        }
        let mut visited = [false; TOTAL_SEGMENTS];
        let mut parent = vec![usize::MAX; TOTAL_SEGMENTS];
        let mut queue = VecDeque::new();
        visited[src] = true;
        queue.push_back(src);
        while let Some(cur) = queue.pop_front() {
            for &next in &self.adj[cur] {
                if !visited[next] {
                    visited[next] = true;
                    parent[next] = cur;
                    if next == dst {
                        // Reconstruct
                        let mut path = Vec::new();
                        let mut n = dst;
                        while n != src {
                            path.push(n);
                            n = parent[n];
                        }
                        path.reverse();
                        return Some(path);
                    }
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// Try to claim a segment for an AGV. Returns false if occupied.
    pub fn claim(&mut self, seg: SegmentId, agv: AgvId) -> bool {
        if self.occupant[seg].is_some() {
            return false;
        }
        self.occupant[seg] = Some(agv);
        true
    }

    /// Release a segment.
    pub fn release(&mut self, seg: SegmentId) {
        self.occupant[seg] = None;
    }

    /// Who occupies a segment?
    pub fn occupant(&self, seg: SegmentId) -> Option<AgvId> {
        self.occupant[seg]
    }

    pub fn occupancy(&self) -> &[Option<AgvId>] {
        &self.occupant
    }
}

// ── Wait-for graph (deadlock detection) ─────────────────────────────
/// Tracks which AGV is waiting for which other AGV. A cycle in this
/// graph means deadlock.
#[derive(Debug, Default)]
pub struct WaitForGraph {
    /// agv_a → agv_b means "a is waiting for b to release a segment."
    edges: HashMap<AgvId, AgvId>,
}

impl WaitForGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.edges.clear();
    }

    pub fn add_wait(&mut self, waiter: AgvId, blocker: AgvId) {
        self.edges.insert(waiter, blocker);
    }

    pub fn remove(&mut self, agv: AgvId) {
        self.edges.remove(&agv);
    }

    /// Detect a cycle. Returns the set of AGVs involved, or empty.
    pub fn find_cycle(&self) -> Vec<AgvId> {
        let mut visited = HashSet::new();
        for &start in self.edges.keys() {
            let mut path = Vec::new();
            let mut seen = HashSet::new();
            let mut cur = start;
            loop {
                if seen.contains(&cur) {
                    // Found a cycle — extract it.
                    let pos = path.iter().position(|&x| x == cur).unwrap();
                    return path[pos..].to_vec();
                }
                if visited.contains(&cur) {
                    break;
                }
                seen.insert(cur);
                path.push(cur);
                if let Some(&next) = self.edges.get(&cur) {
                    cur = next;
                } else {
                    break;
                }
            }
            visited.extend(seen);
        }
        Vec::new()
    }
}
