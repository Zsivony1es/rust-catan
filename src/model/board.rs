//! Board model: serializable DTO plus pure validators.
//!
//! The API transfers [`BoardState`] (plain ids, no graph library). All
//! adjacency is derived from the stored ids; the board is tiny (19 hexes,
//! 54 intersections, 72 edges) so on-the-fly scans are plenty fast.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::enums::{FieldType, PlayerColor, Resource};
use super::hex::Hex;

pub type HexId = u32;
pub type IntersectionId = u32;
pub type EdgeId = u32;

/// Number of intersections / edges of a radius-2 (19 hex) Catan board.
pub const INTERSECTION_COUNT: usize = 54;
pub const EDGE_COUNT: usize = 72;
pub const HEX_COUNT: usize = 19;

/// Settlement or city placed on an intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Building {
    pub owner: PlayerColor,
    pub kind: BuildingKind,
}

/// Kind of building on an intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildingKind {
    Settlement,
    City,
}

/// A board corner where up to 3 hexes meet. Settlements/cities live here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intersection {
    pub id: IntersectionId,
    /// Hexes touching this corner (1-3).
    pub hexes: Vec<HexId>,
    pub building: Option<Building>,
}

/// A side between two intersections. Roads live here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub a: IntersectionId,
    pub b: IntersectionId,
    pub road: Option<PlayerColor>,
}

/// Port exchange rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    /// 3:1 for any resource.
    Generic,
    /// 2:1 for the given resource.
    Specific(Resource),
}

impl PortKind {
    /// How many cards of one resource this port demands.
    #[must_use]
    pub const fn give_amount(self) -> u8 {
        match self {
            PortKind::Generic => 3,
            PortKind::Specific(_) => 2,
        }
    }
}

/// A harbor on the coast, usable via a settlement/city on either endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub id: u32,
    pub edge_id: EdgeId,
    pub intersections: [IntersectionId; 2],
    pub kind: PortKind,
}

/// Full board state transferred as JSON between server and client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardState {
    pub hexes: Vec<Hex>,
    pub intersections: Vec<Intersection>,
    pub edges: Vec<Edge>,
    pub ports: Vec<Port>,
}

impl BoardState {
    #[must_use]
    pub fn intersection(&self, id: IntersectionId) -> Option<&Intersection> {
        self.intersections.iter().find(|i| i.id == id)
    }

    pub fn intersection_mut(&mut self, id: IntersectionId) -> Option<&mut Intersection> {
        self.intersections.iter_mut().find(|i| i.id == id)
    }

    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.iter().find(|e| e.id == id)
    }

    pub fn edge_mut(&mut self, id: EdgeId) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|e| e.id == id)
    }

    #[must_use]
    pub fn hex(&self, id: HexId) -> Option<&Hex> {
        self.hexes.iter().find(|h| h.id == id)
    }

    /// Intersections adjacent to `v`.
    #[must_use]
    pub fn neighbors(&self, v: IntersectionId) -> Vec<IntersectionId> {
        self.edges
            .iter()
            .filter_map(|e| {
                if e.a == v {
                    Some(e.b)
                } else if e.b == v {
                    Some(e.a)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Edges incident to `v`.
    #[must_use]
    pub fn incident_edges(&self, v: IntersectionId) -> Vec<EdgeId> {
        self.edges
            .iter()
            .filter(|e| e.a == v || e.b == v)
            .map(|e| e.id)
            .collect()
    }

    /// Edge directly connecting `a` and `b`, if any.
    #[must_use]
    pub fn edge_between(&self, a: IntersectionId, b: IntersectionId) -> Option<EdgeId> {
        self.edges
            .iter()
            .find(|e| (e.a == a && e.b == b) || (e.a == b && e.b == a))
            .map(|e| e.id)
    }

    /// Hexes adjacent to both endpoints (used for robber victim lookup).
    #[must_use]
    pub fn hexes_adjacent_to(&self, v: IntersectionId) -> Vec<HexId> {
        self.intersection(v)
            .map(|i| i.hexes.clone())
            .unwrap_or_default()
    }

    /// Distance rule: `v` is empty and no neighboring intersection is built on.
    #[must_use]
    pub fn distance_rule_ok(&self, v: IntersectionId) -> bool {
        let Some(node) = self.intersection(v) else {
            return false;
        };
        if node.building.is_some() {
            return false;
        }
        self.neighbors(v).iter().all(|n| {
            self.intersection(*n)
                .is_none_or(|node| node.building.is_none())
        })
    }

    /// Whether `color` may build a road on `edge_id`.
    ///
    /// The edge must be empty and touch `color`'s network: one endpoint holds
    /// their building, or an incident own road reaches it through a vertex
    /// that is not occupied by an opponent (roads cannot extend past an
    /// opponent's building).
    #[must_use]
    pub fn road_spot_ok(&self, color: PlayerColor, edge_id: EdgeId) -> bool {
        let Some(edge) = self.edge(edge_id) else {
            return false;
        };
        if edge.road.is_some() {
            return false;
        }
        for end in [edge.a, edge.b] {
            let Some(node) = self.intersection(end) else {
                continue;
            };
            if node.building.is_some_and(|b| b.owner == color) {
                return true;
            }
            // Opponent buildings block extension past this vertex.
            if node.building.is_some_and(|b| b.owner != color) {
                continue;
            }
            let reaches = self
                .edges
                .iter()
                .any(|e| e.road == Some(color) && (e.a == end || e.b == end) && e.id != edge_id);
            if reaches {
                return true;
            }
        }
        false
    }

    /// Whether `color` may found a settlement on `v`.
    ///
    /// Always enforces emptiness + the distance rule. When
    /// `require_road_connection` is set (normal play, not setup), `v` must
    /// also touch one of `color`'s roads.
    #[must_use]
    pub fn settlement_spot_ok(
        &self,
        color: PlayerColor,
        v: IntersectionId,
        require_road_connection: bool,
    ) -> bool {
        if !self.distance_rule_ok(v) {
            return false;
        }
        if !require_road_connection {
            return true;
        }
        self.edges
            .iter()
            .any(|e| e.road == Some(color) && (e.a == v || e.b == v))
    }

    /// Length (in roads) of `color`'s longest continuous route.
    ///
    /// Opponent settlements/cities in the middle of a line split it: paths
    /// may end at, but never pass through, an opponent-occupied vertex.
    /// Edges are never reused within one path.
    #[must_use]
    pub fn longest_road(&self, color: PlayerColor) -> usize {
        let owned: HashSet<EdgeId> = self
            .edges
            .iter()
            .filter(|e| e.road == Some(color))
            .map(|e| e.id)
            .collect();
        if owned.is_empty() {
            return 0;
        }
        let blocked: HashSet<IntersectionId> = self
            .intersections
            .iter()
            .filter(|i| i.building.is_some_and(|b| b.owner != color))
            .map(|i| i.id)
            .collect();

        // Adjacency restricted to owned roads: vertex -> [(neighbor, edge)].
        let mut adj: HashMap<IntersectionId, Vec<(IntersectionId, EdgeId)>> = HashMap::new();
        for edge in self.edges.iter().filter(|e| owned.contains(&e.id)) {
            adj.entry(edge.a).or_default().push((edge.b, edge.id));
            adj.entry(edge.b).or_default().push((edge.a, edge.id));
        }

        fn dfs(
            current: IntersectionId,
            adj: &HashMap<IntersectionId, Vec<(IntersectionId, EdgeId)>>,
            blocked: &HashSet<IntersectionId>,
            visited: &mut HashSet<EdgeId>,
        ) -> usize {
            // A blocked vertex ends the path (it was entered, not passed through).
            if blocked.contains(&current) {
                return 0;
            }
            let mut best = 0;
            if let Some(nexts) = adj.get(&current) {
                for (next, edge) in nexts {
                    if visited.contains(edge) {
                        continue;
                    }
                    visited.insert(*edge);
                    // Stepping onto a blocked vertex counts the edge but stops.
                    let len = if blocked.contains(next) {
                        1
                    } else {
                        1 + dfs(*next, adj, blocked, visited)
                    };
                    best = best.max(len);
                    visited.remove(edge);
                }
            }
            best
        }

        let mut best = 0;
        let mut starts: HashSet<IntersectionId> = HashSet::new();
        for edge in self.edges.iter().filter(|e| owned.contains(&e.id)) {
            starts.insert(edge.a);
            starts.insert(edge.b);
        }
        for start in starts {
            if blocked.contains(&start) {
                continue;
            }
            best = best.max(dfs(start, &adj, &blocked, &mut HashSet::new()));
        }
        best
    }

    /// Ports `color` can currently use (building on either endpoint).
    #[must_use]
    pub fn ports_for(&self, owned: &[IntersectionId], cities: &[IntersectionId]) -> Vec<&Port> {
        self.ports
            .iter()
            .filter(|p| {
                owned.contains(&p.intersections[0])
                    || owned.contains(&p.intersections[1])
                    || cities.contains(&p.intersections[0])
                    || cities.contains(&p.intersections[1])
            })
            .collect()
    }

    /// Fixed beginner-style layout: 19 hexes with the standard terrain mix
    /// and number tokens, 54 intersections, 72 edges, 9 ports, robber on the
    /// desert. Deterministic so tests and replays are stable.
    #[must_use]
    pub fn fixed_setup() -> Self {
        // Terrain per hex id (axial order, desert at the center id 9).
        const TERRAIN: [FieldType; HEX_COUNT] = [
            FieldType::Forest,    // 0
            FieldType::Pasture,   // 1
            FieldType::Fields,    // 2
            FieldType::Hills,     // 3
            FieldType::Mountains, // 4
            FieldType::Fields,    // 5
            FieldType::Pasture,   // 6
            FieldType::Forest,    // 7
            FieldType::Hills,     // 8
            FieldType::Desert,    // 9 (center)
            FieldType::Mountains, // 10
            FieldType::Pasture,   // 11
            FieldType::Fields,    // 12
            FieldType::Forest,    // 13
            FieldType::Hills,     // 14
            FieldType::Pasture,   // 15
            FieldType::Fields,    // 16
            FieldType::Forest,    // 17
            FieldType::Mountains, // 18
        ];
        // 18 number tokens in hex-id order (skipping the desert).
        const NUMBERS: [u8; 18] = [5, 2, 6, 3, 8, 10, 9, 12, 11, 4, 8, 10, 9, 4, 5, 6, 3, 11];

        // --- Hex centers: axial coords with |q|,|r|,|s| <= 2, sorted by (r,q).
        let mut axial: Vec<(i32, i32)> = Vec::new();
        for q in -2i32..=2 {
            for r in -2i32..=2 {
                let s = -q - r;
                if q.abs().max(r.abs()).max(s.abs()) <= 2 {
                    axial.push((q, r));
                }
            }
        }
        axial.sort_by_key(|(q, r)| (*r, *q));
        debug_assert_eq!(axial.len(), HEX_COUNT);

        // --- Corners (pointy-top hexes), deduplicated by rounded position.
        let sqrt3 = 3f64.sqrt();
        let mut vertex_key: HashMap<(i64, i64), usize> = HashMap::new();
        let mut vertex_pos: Vec<(f64, f64)> = Vec::new();
        let mut vertex_hexes: Vec<Vec<HexId>> = Vec::new();
        // Per hex: the 6 corner vertex indices in winding order.
        let mut hex_corners: Vec<[usize; 6]> = Vec::new();
        for (hi, (q, r)) in axial.iter().enumerate() {
            let cx = sqrt3 * (f64::from(*q) + f64::from(*r) / 2.0);
            let cy = 1.5 * f64::from(*r);
            let mut corners = [0usize; 6];
            for (k, slot) in corners.iter_mut().enumerate() {
                let angle = std::f64::consts::PI / 180.0 * (30.0 + 60.0 * f64::from(k as u8));
                let x = cx + angle.cos();
                let y = cy + angle.sin();
                let key = ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64);
                let idx = *vertex_key.entry(key).or_insert_with(|| {
                    vertex_pos.push((x, y));
                    vertex_hexes.push(Vec::new());
                    vertex_pos.len() - 1
                });
                if !vertex_hexes[idx].contains(&(hi as u32)) {
                    vertex_hexes[idx].push(hi as u32);
                }
                *slot = idx;
            }
            hex_corners.push(corners);
        }
        debug_assert_eq!(vertex_pos.len(), INTERSECTION_COUNT);

        // --- Edges: hex sides deduplicated by sorted vertex pair.
        let mut edge_key: BTreeMap<(usize, usize), Vec<HexId>> = BTreeMap::new();
        for (hi, corners) in hex_corners.iter().enumerate() {
            for k in 0..6 {
                let a = corners[k];
                let b = corners[(k + 1) % 6];
                edge_key
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push(hi as u32);
            }
        }
        debug_assert_eq!(edge_key.len(), EDGE_COUNT);

        // Deterministic intersection ids: sort vertices by (y, x).
        let mut order: Vec<usize> = (0..vertex_pos.len()).collect();
        order.sort_by(|a, b| {
            vertex_pos[*a]
                .1
                .partial_cmp(&vertex_pos[*b].1)
                .expect("finite coords")
                .then(
                    vertex_pos[*a]
                        .0
                        .partial_cmp(&vertex_pos[*b].0)
                        .expect("finite coords"),
                )
        });
        let mut remap = vec![0u32; vertex_pos.len()];
        for (new_id, old) in order.iter().enumerate() {
            remap[*old] = new_id as u32;
        }

        let mut intersections: Vec<Intersection> = order
            .iter()
            .enumerate()
            .map(|(new_id, old)| {
                let mut hexes = vertex_hexes[*old].clone();
                hexes.sort_unstable();
                Intersection {
                    id: new_id as u32,
                    hexes,
                    building: None,
                }
            })
            .collect();
        intersections.sort_by_key(|i| i.id);

        // Deterministic edge ids: sort by (min endpoint, max endpoint).
        let mut edge_list: Vec<(u32, u32)> = edge_key
            .keys()
            .map(|(a, b)| (remap[*a], remap[*b]))
            .map(|(a, b)| (a.min(b), a.max(b)))
            .collect();
        edge_list.sort_unstable();
        let edges: Vec<Edge> = edge_list
            .into_iter()
            .enumerate()
            .map(|(id, (a, b))| Edge {
                id: id as u32,
                a,
                b,
                road: None,
            })
            .collect();

        // Hex adjacency via shared edges (for number-token de-clumping).
        let mut hex_adj: Vec<HashSet<HexId>> = (0..HEX_COUNT).map(|_| HashSet::new()).collect();
        for hexes in edge_key.values() {
            if hexes.len() == 2 {
                hex_adj[hexes[0] as usize].insert(hexes[1]);
                hex_adj[hexes[1] as usize].insert(hexes[0]);
            }
        }

        // Assign numbers, then break up adjacent 6/8 pairs deterministically.
        let mut numbers: Vec<Option<u8>> = vec![None; HEX_COUNT];
        {
            let mut iter = NUMBERS.iter();
            for (hi, terrain) in TERRAIN.iter().enumerate() {
                if !matches!(terrain, FieldType::Desert) {
                    numbers[hi] = Some(*iter.next().expect("18 numbers"));
                }
            }
        }
        for _ in 0..32 {
            let mut clash: Option<(usize, usize)> = None;
            for a in 0..HEX_COUNT {
                if !matches!(numbers[a], Some(6) | Some(8)) {
                    continue;
                }
                if let Some(b) = hex_adj[a]
                    .iter()
                    .map(|h| *h as usize)
                    .filter(|b| *b > a && matches!(numbers[*b], Some(6) | Some(8)))
                    .min()
                {
                    clash = Some((a, b));
                    break;
                }
            }
            let Some((_, b)) = clash else { break };
            // Swap b's token with the first lower-id non-6/8 token.
            if let Some(c) =
                (0..HEX_COUNT).find(|c| !matches!(numbers[*c], Some(6) | Some(8) | None))
            {
                numbers.swap(b, c);
            } else {
                break;
            }
        }

        let hexes: Vec<Hex> = TERRAIN
            .iter()
            .enumerate()
            .map(|(hi, field)| Hex::new(hi as u32, *field, numbers[hi]))
            .collect();

        let mut board = Self {
            hexes,
            intersections,
            edges,
            ports: Vec::new(),
        };
        // Real vertex positions per intersection id, for rim-ordered ports.
        let mut pos_by_id = vec![(0.0f64, 0.0f64); vertex_pos.len()];
        for (old, pos) in vertex_pos.iter().enumerate() {
            pos_by_id[remap[old] as usize] = *pos;
        }
        board.ports = board.assign_ports(&edge_key, &remap, &pos_by_id);
        board
    }

    /// Place the 9 ports (4 generic 3:1 + 5 specific 2:1) on evenly spaced
    /// coastal edges, in rim order.
    fn assign_ports(
        &self,
        edge_key: &BTreeMap<(usize, usize), Vec<HexId>>,
        remap: &[u32],
        pos_by_id: &[(f64, f64)],
    ) -> Vec<Port> {
        // Coastal edges touch exactly one hex.
        let mut coastal: Vec<(EdgeId, f64)> = Vec::new();
        for ((a, b), hexes) in edge_key {
            if hexes.len() != 1 {
                continue;
            }
            let (ra, rb) = (remap[*a], remap[*b]);
            let (lo, hi) = (ra.min(rb), ra.max(rb));
            if let Some(id) = self.edge_between(lo, hi) {
                // Angle of the edge midpoint around the board center.
                let (ax, ay) = pos_by_id[lo as usize];
                let (bx, by) = pos_by_id[hi as usize];
                let angle = ((ay + by) / 2.0).atan2((ax + bx) / 2.0);
                coastal.push((id, angle));
            }
        }
        coastal.sort_by(|a, b| a.1.partial_cmp(&b.1).expect("finite angle"));
        debug_assert_eq!(coastal.len(), 30);

        const KINDS: [PortKind; 9] = [
            PortKind::Generic,
            PortKind::Specific(Resource::Wood),
            PortKind::Generic,
            PortKind::Specific(Resource::Brick),
            PortKind::Generic,
            PortKind::Specific(Resource::Wool),
            PortKind::Generic,
            PortKind::Specific(Resource::Wheat),
            PortKind::Specific(Resource::Ore),
        ];
        (0..9)
            .map(|i| {
                let edge_id = coastal[i * coastal.len() / 9].0;
                let edge = self.edge(edge_id).expect("coastal edge");
                Port {
                    id: i as u32,
                    edge_id,
                    intersections: [edge.a, edge.b],
                    kind: KINDS[i],
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_setup_counts() {
        let board = BoardState::fixed_setup();
        assert_eq!(board.hexes.len(), HEX_COUNT);
        assert_eq!(board.intersections.len(), INTERSECTION_COUNT);
        assert_eq!(board.edges.len(), EDGE_COUNT);
        assert_eq!(board.ports.len(), 9);
    }

    #[test]
    fn fixed_setup_numbers_and_robber() {
        let board = BoardState::fixed_setup();
        let numbers: Vec<u8> = board.hexes.iter().filter_map(|h| h.number).collect();
        assert_eq!(numbers.len(), 18);
        assert!(!numbers.contains(&7));
        let mut sorted = numbers.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12]
        );
        let deserts: Vec<&Hex> = board
            .hexes
            .iter()
            .filter(|h| matches!(h.field, FieldType::Desert))
            .collect();
        assert_eq!(deserts.len(), 1);
        assert_eq!(deserts[0].number, None);
        assert!(deserts[0].has_robber);
        assert_eq!(board.hexes.iter().filter(|h| h.has_robber).count(), 1);
    }

    #[test]
    fn every_edge_connects_two_intersections() {
        let board = BoardState::fixed_setup();
        for edge in &board.edges {
            assert_ne!(edge.a, edge.b);
            assert!(board.intersection(edge.a).is_some());
            assert!(board.intersection(edge.b).is_some());
            // Reverse lookup agrees.
            assert_eq!(board.edge_between(edge.a, edge.b), Some(edge.id));
        }
    }

    #[test]
    fn distance_rule_blocks_neighbors() {
        let mut board = BoardState::fixed_setup();
        let v = board.intersections[20].id;
        assert!(board.distance_rule_ok(v));
        board.intersection_mut(v).expect("exists").building = Some(Building {
            owner: PlayerColor::Red,
            kind: BuildingKind::Settlement,
        });
        assert!(!board.distance_rule_ok(v));
        for n in board.neighbors(v) {
            assert!(!board.distance_rule_ok(n), "neighbor {n}");
        }
    }

    #[test]
    fn longest_road_line_and_branch() {
        let mut board = BoardState::fixed_setup();
        // Build a simple chain of 4 edges: find a path greedily.
        let start = board.intersections[0].id;
        let mut current = start;
        let mut chain: Vec<EdgeId> = Vec::new();
        for _ in 0..4 {
            let next = board
                .incident_edges(current)
                .into_iter()
                .filter_map(|e| {
                    let edge = board.edge(e).expect("edge");
                    let other = if edge.a == current { edge.b } else { edge.a };
                    (!chain.contains(&e)).then_some((e, other))
                })
                .next();
            let Some((eid, other)) = next else { break };
            board.edge_mut(eid).expect("edge").road = Some(PlayerColor::Red);
            chain.push(eid);
            current = other;
        }
        assert_eq!(board.longest_road(PlayerColor::Red), chain.len());
        assert_eq!(board.longest_road(PlayerColor::Blue), 0);
    }

    #[test]
    fn longest_road_broken_by_opponent_settlement() {
        let mut board = BoardState::fixed_setup();
        // Chain of 6 through `mid`; opponent settles on `mid` -> longest 3.
        let start = board.intersections[10].id;
        let mut current = start;
        let mut verts = vec![start];
        for _ in 0..6 {
            let next = board
                .incident_edges(current)
                .into_iter()
                .filter_map(|e| {
                    let edge = board.edge(e).expect("edge");
                    if edge.road.is_some() {
                        return None;
                    }
                    let other = if edge.a == current { edge.b } else { edge.a };
                    (!verts.contains(&other)).then_some((e, other))
                })
                .next();
            let Some((eid, other)) = next else { break };
            board.edge_mut(eid).expect("edge").road = Some(PlayerColor::Red);
            verts.push(other);
            current = other;
        }
        assert!(
            verts.len() >= 7,
            "need a chain of 6, got {}",
            verts.len() - 1
        );
        let mid = verts[3];
        board.intersection_mut(mid).expect("vertex").building = Some(Building {
            owner: PlayerColor::Blue,
            kind: BuildingKind::Settlement,
        });
        assert_eq!(board.longest_road(PlayerColor::Red), 3);
    }
}
