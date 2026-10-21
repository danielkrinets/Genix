//! Recover selectable lines, circles and open circular arcs from sharp mesh edges.
use fusion_core::{Mesh, doc::EdgeRef};
use glam::DVec3;
use std::collections::{HashMap,HashSet};

pub fn selectable(mesh: &Mesh) -> Vec<EdgeRef> {
    let segs=mesh.sharp_edges(30.0);
    let pos=|v:u32| DVec3::from(mesh.vertices[v as usize]);
    let mut at:HashMap<u32,Vec<usize>>=HashMap::new();
    for (i,&(a,b)) in segs.iter().enumerate() { at.entry(a).or_default().push(i); at.entry(b).or_default().push(i); }
    let other=|i:usize,v:u32| if segs[i].0==v {segs[i].1} else {segs[i].0};
    let mut used=HashSet::new();
    let mut out=Vec::new();
    // A local triple gives an unbiased centre even for a semicircle. Grow only along
    // connected edges that lie on that same circle; tangent straight runs stay separate.
    for (&v,list) in &at {
        for a in 0..list.len() { for b in a+1..list.len() {
            let (i,j)=(list[a],list[b]);
            if used.contains(&i) || used.contains(&j) {continue;}
            let p=pos(v); let u=pos(other(i,v))-p; let w=pos(other(j,v))-p;
            let cosine=(-u.normalize_or_zero()).dot(w.normalize_or_zero());
            if !(0.7..0.9999999).contains(&cosine) {continue;}
            let n=u.cross(w); let n2=n.length_squared();
            if n2<1e-16 {continue;}
            let center=p+(w.cross(n)*u.length_squared()+n.cross(u)*w.length_squared())/(2.0*n2);
            let axis=n.normalize(); let radius=p.distance(center); let tol=1e-5*(1.0+radius);
            let on=|q:DVec3| (q.distance(center)-radius).abs()<tol && (q-center).dot(axis).abs()<tol;
            let mut group=HashSet::new(); let mut todo=vec![i];
            while let Some(k)=todo.pop() {
                if used.contains(&k) || group.contains(&k) {continue;}
                let (a,b)=segs[k];
                if !on(pos(a)) || !on(pos(b)) || (pos(a)-center).angle_between(pos(b)-center)>std::f64::consts::FRAC_PI_4 {continue;}
                group.insert(k);
                todo.extend(at[&a].iter().copied()); todo.extend(at[&b].iter().copied());
            }
            if group.len()<4 {continue;}
            let mut degree:HashMap<u32,Vec<usize>>=HashMap::new();
            for &k in &group {for v in [segs[k].0,segs[k].1] {degree.entry(v).or_default().push(k);}}
            if degree.values().any(|x| x.len()>2) {continue;}
            let ends:Vec<u32>=degree.iter().filter(|(_,x)| x.len()==1).map(|(&v,_)|v).collect();
            if !ends.is_empty() && ends.len()!=2 {continue;}
            let start=*ends.iter().min().or_else(||degree.keys().min()).unwrap();
            let mut chain=vec![pos(start)]; let mut vertex=start; let mut seen=HashSet::new();
            loop {
                let Some(&k)=degree[&vertex].iter().find(|k| !seen.contains(*k)) else {break};
                seen.insert(k); vertex=other(k,vertex); chain.push(pos(vertex));
                if vertex==start {break;}
            }
            if seen.len()!=group.len() {continue;}
            let sign=axis.dot((chain[0]-center).cross(chain[1]-center)).signum();
            let axis=axis*sign;
            let sweep:f64=chain.windows(2).map(|w| (w[0]-center).angle_between(w[1]-center)).sum::<f64>().to_degrees();
            if sweep<10.0 {continue;}
            let mut edge=EdgeRef::round(chain[0].to_array(),center.to_array(),axis.to_array());
            if !ends.is_empty() {edge.sweep=Some(sweep);}
            out.push(edge); used.extend(group);
        }}
    }
    // Merge collinear pieces without discarding lines tangent to an arc.
    for seed in 0..segs.len() {
        if used.contains(&seed) {continue;}
        let anchor=pos(segs[seed].0); let dir=(pos(segs[seed].1)-anchor).normalize_or_zero();
        let mut todo=vec![seed]; let mut pts=Vec::new();
        while let Some(k)=todo.pop() {
            if used.contains(&k) {continue;}
            let (a,b)=(pos(segs[k].0),pos(segs[k].1));
            if (a-anchor).cross(dir).length()>1e-5 || (b-anchor).cross(dir).length()>1e-5 {continue;}
            used.insert(k); pts.extend([a,b]);
            todo.extend(at[&segs[k].0].iter().copied()); todo.extend(at[&segs[k].1].iter().copied());
        }
        if let (Some(a),Some(b))=(pts.iter().min_by(|a,b| a.dot(dir).total_cmp(&b.dot(dir))),pts.iter().max_by(|a,b| a.dot(dir).total_cmp(&b.dot(dir)))) {
            out.push(EdgeRef::line(a.to_array(),b.to_array()));
        }
    }
    out.sort_by(|a,b| a.a.partial_cmp(&b.a).unwrap().then(a.b.partial_cmp(&b.b).unwrap()));
    out
}

/// End points of an edge with the direction pointing away from the edge there (a line has two, an open arc two, a full circle none).
fn ends(e: &EdgeRef) -> Vec<(DVec3, DVec3)> {
    if !e.circle {
        let (a, b) = (DVec3::from(e.a), DVec3::from(e.b));
        return vec![(a, (a - b).normalize_or_zero()), (b, (b - a).normalize_or_zero())];
    }
    let Some(sweep) = e.sweep else { return Vec::new() };
    let (c, axis, p0) = (DVec3::from(e.b), DVec3::from(e.axis).normalize_or_zero(), DVec3::from(e.a));
    let r0 = p0 - c;
    let r1 = glam::DQuat::from_axis_angle(axis, sweep.to_radians()) * r0;
    vec![(p0, -axis.cross(r0).normalize_or_zero()), (c + r1, axis.cross(r1).normalize_or_zero())]
}

/// The edge together with every edge that continues it without a corner (a straight rim running into a rounded one and on):
/// the «tangent chain» that a fillet has to take as a whole, otherwise the rounded corners stay as sharp ears.
pub fn tangent_chain(all: &[EdgeRef], start: &EdgeRef) -> Vec<EdgeRef> {
    let tol = |e: &EdgeRef| 1e-3 * (1.0 + DVec3::from(e.a).distance(DVec3::from(e.b)));
    let same = |x: &EdgeRef, y: &EdgeRef| x.circle == y.circle && DVec3::from(x.a).distance(DVec3::from(y.a)) < 1e-6 && DVec3::from(x.b).distance(DVec3::from(y.b)) < 1e-6;
    let mut chain = vec![*start];
    let mut i = 0;
    while i < chain.len() {
        let current = chain[i];
        for (p, t) in ends(&current) {
            for other in all {
                if chain.iter().any(|c| same(c, other)) {
                    continue;
                }
                let joins = ends(other).into_iter().any(|(q, u)| q.distance(p) < tol(&current).max(tol(other)) && t.dot(u) < -0.99);
                if joins {
                    chain.push(*other);
                }
            }
        }
        i += 1;
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn capsule() -> Mesh {
        let mut outer=Vec::new();
        for (cx,start) in [(10.0,-90.0_f64),(-10.0,90.0_f64)] {
            for i in 0..=24 { let t=(start+i as f64*180.0/24.0).to_radians(); outer.push([cx+5.0*t.cos(),5.0*t.sin()]); }
        }
        fusion_core::geom::extrude_region(&fusion_core::Plane::XY,&fusion_core::Region {outer,holes:vec![],ids:vec![]},0.0,5.0).unwrap()
    }
    #[test]
    fn a_straight_rim_chains_through_the_rounded_corners_around_the_whole_top() {
        // 40 x 20 plate with 5 mm rounded corners, 5 mm thick
        let mut outer = Vec::new();
        for (cx, cy, start) in [(35.0, 5.0, -90.0_f64), (35.0, 15.0, 0.0), (5.0, 15.0, 90.0), (5.0, 5.0, 180.0)] {
            for i in 0..=12 {
                let t = (start + i as f64 * 90.0 / 12.0).to_radians();
                outer.push([cx + 5.0 * t.cos(), cy + 5.0 * t.sin()]);
            }
        }
        let mesh = fusion_core::geom::extrude_region(&fusion_core::Plane::XY, &fusion_core::Region { outer, holes: vec![], ids: vec![] }, 0.0, 5.0).unwrap();
        let all = selectable(&mesh);
        let start = all.iter().find(|e| !e.circle && e.a[2] > 4.9 && e.b[2] > 4.9).expect("a straight top rim");
        let chain = tangent_chain(&all, start);
        assert_eq!(chain.len(), 8, "four straight rims and four rounded corners: {chain:?}");
        assert!(chain.iter().all(|e| if e.circle { e.a[2] > 4.9 } else { e.a[2] > 4.9 && e.b[2] > 4.9 }), "the chain stays on the top");
        // the bottom rim and the vertical edges are separate chains
        assert!(all.len() > chain.len());
    }

    #[test]
    fn the_top_rim_of_a_box_with_rounded_vertical_edges_is_one_chain_of_eight() {
        use fusion_core::doc::{Document, FeatureKind, Shape};
        let n = fusion_core::Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(40.0) }, at: [n(0.0), n(0.0), n(0.0)], op: fusion_core::Op::NewBody });
        let mut ev = fusion_core::Evaluator::default();
        ev.evaluate(&d);
        let body = ev.model().bodies[0].id;
        let vertical: Vec<EdgeRef> = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)].iter().map(|(x, y)| EdgeRef::line([*x, *y, 0.0], [*x, *y, 40.0])).collect();
        d.insert(FeatureKind::Fillet { body, edges: vertical, radius: n(5.0) });
        ev.evaluate(&d);
        let mesh = ev.model().bodies[0].mesh().clone();
        let all = selectable(&mesh);
        let top_lines = all.iter().filter(|e| !e.circle && e.a[2] > 39.9 && e.b[2] > 39.9).count();
        let top_arcs = all.iter().filter(|e| e.circle && e.a[2] > 39.9).count();
        assert_eq!((top_lines, top_arcs), (4, 4), "selectable edges: {all:?}");
        let start = all.iter().find(|e| !e.circle && e.a[2] > 39.9 && e.b[2] > 39.9).unwrap();
        assert_eq!(tangent_chain(&all, start).len(), 8);
    }

    #[test]
    fn diameter_does_not_turn_a_semicircular_rim_into_a_full_circle() {
        let outer=(0..=24).map(|i| {let t=i as f64*std::f64::consts::PI/24.0;[5.0*t.cos(),5.0*t.sin()]}).collect();
        let mesh=fusion_core::geom::extrude_region(&fusion_core::Plane::XY,&fusion_core::Region{outer,holes:vec![],ids:vec![]},0.0,5.0).unwrap();
        let edges=selectable(&mesh);
        assert_eq!(edges.iter().filter(|e|e.sweep.is_some_and(|a|(a-180.0).abs()<0.01)).count(),2,"{edges:?}");
        assert!(!edges.iter().any(|e|e.circle && e.sweep.is_none()));
    }

    #[test]
    fn capsule_has_four_semicircular_rims_and_tangent_lines() {
        let edges=selectable(&capsule());
        let arcs:Vec<_>=edges.iter().filter(|e|e.sweep.is_some()).collect();
        assert_eq!(arcs.len(),4,"{edges:?}");
        assert!(arcs.iter().all(|e| (e.sweep.unwrap()-180.0).abs()<0.01));
        assert!(edges.iter().filter(|e|!e.circle).count()>=4);
        for e in arcs {
            let (tool,concave)=fusion_core::geom::circle_tool_span(&capsule(),e.b.into(),e.axis.into(),e.a.into(),fusion_core::geom::EdgeOp::Fillet(0.5),e.sweep.unwrap()).unwrap();
            assert!(!concave); assert!(tool.volume().abs()>0.0);
            let solid=fusion_core::csg::Solid::from_mesh(&capsule()).unwrap();
            let result=solid.boolean(&fusion_core::csg::Solid::from_mesh(&tool).unwrap(),fusion_core::csg::BoolOp::Subtract);
            assert!(result.volume()<solid.volume());
            assert!(result.volume()>solid.volume()*0.95);
        }
    }

    #[test]
    fn adjacent_round_rims_have_an_overlapping_fillet_joint() {
        use fusion_core::csg::{BoolOp, Solid};
        use fusion_core::geom::{circle_tool_span_ext, cylinder_mesh, EdgeOp};
        let a=Solid::from_mesh(&cylinder_mesh(glam::DVec3::new(-4.0,0.0,0.0),6.0,5.0)).unwrap();
        let b=Solid::from_mesh(&cylinder_mesh(glam::DVec3::new(4.0,0.0,0.0),6.0,5.0)).unwrap();
        let solid=a.boolean(&b,BoolOp::Union);
        let arcs:Vec<_>=selectable(&solid.mesh).into_iter().filter(|e|e.circle && e.sweep.is_some() && e.a[2]>4.9).collect();
        let pair=arcs.iter().enumerate().find_map(|(i,a)| arcs.iter().skip(i+1).find(|b| {
            let pa=a.polyline();let pb=b.polyline();
            [pa[0],*pa.last().unwrap()].iter().any(|p|[pb[0],*pb.last().unwrap()].iter().any(|q|p.distance(*q)<0.5))
        }).map(|b|(a,b))).unwrap_or_else(|| panic!("two top arcs share the lobe intersection: {arcs:?}"));
        let tool=|e:&EdgeRef| circle_tool_span_ext(&solid.mesh,e.b.into(),e.axis.into(),e.a.into(),EdgeOp::Fillet(0.5),e.sweep.unwrap(),12.0).unwrap().0;
        let joint=Solid::from_mesh(&tool(pair.0)).unwrap().boolean(&Solid::from_mesh(&tool(pair.1)).unwrap(),BoolOp::Intersect);
        assert!(!joint.is_empty());
        assert!(joint.volume()>1e-6);
    }
}
