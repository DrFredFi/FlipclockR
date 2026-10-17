//! Procedural meshes, in tile-local units (hour/minute tile width = 1).
//!
//! A tile spans x ∈ [-0.5, 0.5], y ∈ [-A/2, A/2] (A = tile aspect), with its
//! front face at z = 0 and its back at z = -THICKNESS. The hinge axis runs along
//! x at y = 0, z = -THICKNESS / 2.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bytemuck::{Pod, Zeroable};
use flipclock_core::layout::TILE_ASPECT;
use glam::{Vec2, Vec3};

pub const THICKNESS: f32 = 0.1;
pub const SPLIT_GAP: f32 = 0.022;
const OUTER_RADIUS: f32 = 0.075;
const HINGE_RADIUS: f32 = 0.022;
const BEVEL: f32 = 0.016;
const ARC_SEGMENTS: usize = 8;

/// Face kinds, passed to the shader per vertex.
pub const KIND_SIDE: f32 = 0.0;
pub const KIND_FRONT: f32 = 1.0;
pub const KIND_BACK: f32 = 2.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub kind: f32,
}

#[derive(Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    fn push(&mut self, position: Vec3, normal: Vec3, uv: Vec2, kind: f32) -> u32 {
        self.vertices.push(Vertex {
            position: position.into(),
            normal: normal.normalize_or_zero().into(),
            uv: uv.into(),
            kind,
        });
        (self.vertices.len() - 1) as u32
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }
}

/// A point of a 2D outline with its outward normal.
#[derive(Clone, Copy)]
struct OutlinePoint {
    pos: Vec2,
    normal: Vec2,
}

/// Counter-clockwise outline of a rectangle with per-corner radii, shrunk by `inset`.
/// Corner order: bottom-right, top-right, top-left, bottom-left.
fn rounded_rect(min: Vec2, max: Vec2, radii: [f32; 4], inset: f32) -> Vec<OutlinePoint> {
    let corners = [
        (Vec2::new(max.x, min.y), -FRAC_PI_2),
        (Vec2::new(max.x, max.y), 0.0),
        (Vec2::new(min.x, max.y), FRAC_PI_2),
        (Vec2::new(min.x, min.y), PI),
    ];
    let mut points = Vec::new();
    for (i, (corner, start)) in corners.into_iter().enumerate() {
        let r = radii[i];
        let inward = Vec2::new(
            if corner.x == max.x { -1.0 } else { 1.0 },
            if corner.y == max.y { -1.0 } else { 1.0 },
        );
        let center = corner + inward * r;
        for s in 0..=ARC_SEGMENTS {
            let a = start + FRAC_PI_2 * s as f32 / ARC_SEGMENTS as f32;
            let n = Vec2::new(a.cos(), a.sin());
            points.push(OutlinePoint {
                pos: center + n * (r - inset),
                normal: n,
            });
        }
    }
    points
}

/// An extruded, bevelled slab with the given outline. Front/back faces get
/// texture coordinates over the whole tile: front v = (A/2 - y)/A, back v = (A/2 + y)/A,
/// so a top flap rotated 180° shows the lower half of the atlas cell on its back.
fn slab(min: Vec2, max: Vec2, radii: [f32; 4]) -> MeshData {
    let mut m = MeshData::default();
    let outer = rounded_rect(min, max, radii, 0.0);
    let inner = rounded_rect(min, max, radii, BEVEL);
    let n = outer.len();
    let half = TILE_ASPECT / 2.0;
    let front_uv = |p: Vec2| Vec2::new(p.x + 0.5, (half - p.y) / TILE_ASPECT);
    let back_uv = |p: Vec2| Vec2::new(p.x + 0.5, (half + p.y) / TILE_ASPECT);

    let z_front = 0.0;
    let z_back = -THICKNESS;
    let z_front_edge = -BEVEL;
    let z_back_edge = -THICKNESS + BEVEL;

    // Front face (fan around the center; the outline is convex).
    let center = (min + max) / 2.0;
    let c = m.push(
        center.extend(z_front),
        Vec3::Z,
        front_uv(center),
        KIND_FRONT,
    );
    let front: Vec<u32> = inner
        .iter()
        .map(|p| m.push(p.pos.extend(z_front), Vec3::Z, front_uv(p.pos), KIND_FRONT))
        .collect();
    for i in 0..n {
        m.tri(c, front[i], front[(i + 1) % n]);
    }

    // Back face, wound the other way.
    let c = m.push(center.extend(z_back), -Vec3::Z, back_uv(center), KIND_BACK);
    let back: Vec<u32> = inner
        .iter()
        .map(|p| m.push(p.pos.extend(z_back), -Vec3::Z, back_uv(p.pos), KIND_BACK))
        .collect();
    for i in 0..n {
        m.tri(c, back[(i + 1) % n], back[i]);
    }

    // Bevels and side walls.
    let ring = |m: &mut MeshData, pts: &[OutlinePoint], z: f32, nz: f32| -> Vec<u32> {
        pts.iter()
            .map(|p| m.push(p.pos.extend(z), p.normal.extend(nz), Vec2::ZERO, KIND_SIDE))
            .collect()
    };
    let front_bevel_in = ring(&mut m, &inner, z_front, 1.0);
    let front_bevel_out = ring(&mut m, &outer, z_front_edge, 1.0);
    let wall_front = ring(&mut m, &outer, z_front_edge, 0.0);
    let wall_back = ring(&mut m, &outer, z_back_edge, 0.0);
    let back_bevel_out = ring(&mut m, &outer, z_back_edge, -1.0);
    let back_bevel_in = ring(&mut m, &inner, z_back, -1.0);
    for i in 0..n {
        let j = (i + 1) % n;
        m.quad(
            front_bevel_out[i],
            front_bevel_out[j],
            front_bevel_in[j],
            front_bevel_in[i],
        );
        m.quad(wall_back[i], wall_back[j], wall_front[j], wall_front[i]);
        m.quad(
            back_bevel_in[i],
            back_bevel_in[j],
            back_bevel_out[j],
            back_bevel_out[i],
        );
    }
    m
}

/// Upper half of a tile (also the moving flap).
pub fn top_half() -> MeshData {
    let half = TILE_ASPECT / 2.0;
    slab(
        Vec2::new(-0.5, SPLIT_GAP / 2.0),
        Vec2::new(0.5, half),
        [HINGE_RADIUS, OUTER_RADIUS, OUTER_RADIUS, HINGE_RADIUS],
    )
}

/// Lower half of a tile.
pub fn bottom_half() -> MeshData {
    let half = TILE_ASPECT / 2.0;
    slab(
        Vec2::new(-0.5, -half),
        Vec2::new(0.5, -SPLIT_GAP / 2.0),
        [OUTER_RADIUS, HINGE_RADIUS, HINGE_RADIUS, OUTER_RADIUS],
    )
}

/// Cylinder along x, from x = -0.5 to 0.5, radius 1.
pub fn cylinder() -> MeshData {
    const SEGMENTS: usize = 20;
    let mut m = MeshData::default();
    let mut side = Vec::new();
    for s in 0..=SEGMENTS {
        let a = TAU * s as f32 / SEGMENTS as f32;
        let n = Vec3::new(0.0, a.cos(), a.sin());
        let l = m.push(Vec3::new(-0.5, n.y, n.z), n, Vec2::ZERO, KIND_SIDE);
        let r = m.push(Vec3::new(0.5, n.y, n.z), n, Vec2::ZERO, KIND_SIDE);
        side.push((l, r));
    }
    for s in 0..SEGMENTS {
        let (l0, r0) = side[s];
        let (l1, r1) = side[s + 1];
        m.quad(l0, r0, r1, l1);
    }
    for (x, normal) in [(-0.5, -Vec3::X), (0.5, Vec3::X)] {
        let c = m.push(Vec3::new(x, 0.0, 0.0), normal, Vec2::ZERO, KIND_SIDE);
        let rim: Vec<u32> = (0..SEGMENTS)
            .map(|s| {
                let a = TAU * s as f32 / SEGMENTS as f32;
                m.push(
                    Vec3::new(x, a.cos(), a.sin()),
                    normal,
                    Vec2::ZERO,
                    KIND_SIDE,
                )
            })
            .collect();
        for s in 0..SEGMENTS {
            let (a, b) = (rim[s], rim[(s + 1) % SEGMENTS]);
            if x > 0.0 {
                m.tri(c, a, b)
            } else {
                m.tri(c, b, a)
            }
        }
    }
    m
}

/// Unit sphere.
pub fn sphere() -> MeshData {
    const RINGS: usize = 16;
    const SEGMENTS: usize = 24;
    let mut m = MeshData::default();
    for r in 0..=RINGS {
        let phi = PI * r as f32 / RINGS as f32;
        for s in 0..=SEGMENTS {
            let theta = TAU * s as f32 / SEGMENTS as f32;
            let n = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
            m.push(n, n, Vec2::ZERO, KIND_SIDE);
        }
    }
    let row = (SEGMENTS + 1) as u32;
    for r in 0..RINGS as u32 {
        for s in 0..SEGMENTS as u32 {
            let a = r * row + s;
            let b = a + row;
            m.quad(a, a + 1, b + 1, b);
        }
    }
    m
}

/// Unit quad in the xy plane facing +z, uv (0,0) at the top-left.
pub fn quad() -> MeshData {
    let mut m = MeshData::default();
    let corners = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
    let ids: Vec<u32> = corners
        .iter()
        .map(|&(x, y)| {
            m.push(
                Vec3::new(x, y, 0.0),
                Vec3::Z,
                Vec2::new(x + 0.5, 0.5 - y),
                KIND_SIDE,
            )
        })
        .collect();
    m.quad(ids[0], ids[1], ids[2], ids[3]);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_meet_at_the_split_gap() {
        let top = top_half();
        let bottom = bottom_half();
        let min_top = top
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
        let max_bottom = bottom
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max);
        assert!((min_top - SPLIT_GAP / 2.0).abs() < 1e-5);
        assert!((max_bottom + SPLIT_GAP / 2.0).abs() < 1e-5);
    }

    #[test]
    fn front_face_uvs_stay_in_the_top_half_of_the_cell() {
        for v in top_half().vertices.iter().filter(|v| v.kind == KIND_FRONT) {
            assert!((0.0..=0.5).contains(&v.uv[1]), "{:?}", v.uv);
        }
    }

    #[test]
    fn back_face_uvs_map_to_the_bottom_half_of_the_cell() {
        for v in top_half().vertices.iter().filter(|v| v.kind == KIND_BACK) {
            assert!((0.5..=1.0).contains(&v.uv[1]), "{:?}", v.uv);
        }
    }

    #[test]
    fn indices_are_in_range() {
        for m in [top_half(), bottom_half(), cylinder(), sphere(), quad()] {
            assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
            assert_eq!(m.indices.len() % 3, 0);
        }
    }
}
