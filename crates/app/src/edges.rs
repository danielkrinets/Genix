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
}
