//! Machine interface for scripts and AI agents: evaluate a document into a JSON report, append a feature given as JSON,
//! export meshes. The desktop binary exposes these as `genix --eval`, `--add`, `--export`, `--new` (see docs/AGENTS.md).
use crate::doc::{Document, Evaluator, FeatureKind};
use serde_json::{json, Value};

fn round(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// Evaluates the whole document: every feature with its error (if any), every body with volume, bounding box and name,
/// and the pairs of bodies that overlap.
pub fn report(doc: &Document) -> Value {
    let mut ev = Evaluator::default();
    ev.evaluate(doc);
    let features: Vec<Value> = doc
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let error = ev.steps().get(i).and_then(|s| s.error.clone());
            json!({ "uid": f.uid, "name": f.name, "type": f.kind.type_label(), "suppressed": f.suppressed, "error": error })
        })
        .collect();
    let model = ev.model();
    let bodies: Vec<Value> = model
        .bodies
        .iter()
        .map(|b| {
            let bounds = b.mesh().bounds().map(|(lo, hi)| json!({ "min": [round(lo.x), round(lo.y), round(lo.z)], "max": [round(hi.x), round(hi.y), round(hi.z)] }));
            json!({ "id": b.id, "name": doc.body_name(b), "volume_mm3": round(b.mesh().volume()), "triangles": b.mesh().triangles.len(), "bounds": bounds })
        })
        .collect();
    let overlaps: Vec<Value> = model.interferences().into_iter().map(|(a, b, v)| json!({ "a": a, "b": b, "volume_mm3": round(v) })).collect();
    let ok = features.iter().all(|f| f["error"].is_null());
    json!({ "ok": ok, "features": features, "bodies": bodies, "interferences": overlaps })
}

/// Appends a feature given as the JSON of `FeatureKind` (the same encoding as inside a .genix file). The document is left
/// unchanged when the new feature fails to evaluate, and the error is returned.
pub fn add_feature(doc: &mut Document, feature_json: &str) -> Result<Value, String> {
    let kind: FeatureKind = serde_json::from_str(feature_json).map_err(|e| format!("feature JSON is not valid: {e}"))?;
    let mut trial = doc.clone();
    let uid = trial.insert(kind);
    let mut ev = Evaluator::default();
    ev.evaluate(&trial);
    if let Some(e) = ev.error(uid) {
        return Err(format!("the feature does not build: {e}"));
    }
    *doc = trial;
    let mut r = report(doc);
    r["added_uid"] = json!(uid);
    Ok(r)
}

/// Feature kinds an agent can add, each with a minimal example in the exact JSON form `add_feature` takes.
pub fn catalog() -> Value {
    use crate::doc::{Op, Shape};
    use crate::Expr;
    let n = Expr::num;
    let example = |k: FeatureKind| serde_json::to_value(k).unwrap_or(Value::Null);
    json!({
        "note": "Expressions are strings or numbers in millimetres/degrees and may reference named parameters; ids of bodies come from `--eval`.",
        "examples": {
            "Primitive/Box": example(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody }),
            "Primitive/Cylinder": example(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(8.0), h: n(20.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody }),
            "Draft": example(FeatureKind::Draft { bodies: vec![1], axis: crate::doc::Axis3::Z, neutral: n(0.0), angle: n(5.0) }),
            "Mirror": example(FeatureKind::Mirror { bodies: vec![1], normal: crate::doc::Axis3::X, join: false, offset: n(0.0) }),
        },
        "types": ["Sketch", "Extrude", "Revolve", "Primitive", "Move", "Mirror", "Split", "Import", "Draft", "Thread", "Pattern", "Combine", "Remove", "Hole", "Coil", "Pipe", "Sweep", "Loft", "Scale", "Chamfer", "Fillet", "OffsetPlane", "AnglePlane", "PlaceOnPlane", "PressPull", "Shell"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_can_build_and_check_a_model_step_by_step() {
        let mut doc = Document::default();
        let box_json = r#"{"Primitive":{"shape":{"Box":{"w":"40","d":"30","h":"10"}},"at":["0","0","0"],"op":"NewBody"}}"#;
        // the encoding of Expr is internal: take the exact JSON from the catalog instead of guessing it
        let example = catalog()["examples"]["Primitive/Box"].to_string();
        let _ = box_json;
        let r = add_feature(&mut doc, &example).unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(r["bodies"].as_array().unwrap().len(), 1);
        assert_eq!(r["bodies"][0]["volume_mm3"], 12000.0);
        let id = r["bodies"][0]["id"].as_u64().unwrap();
        // a draft on the body that was just created, built from the catalog example with the right body id
        let mut draft = catalog()["examples"]["Draft"].clone();
        draft["Draft"]["bodies"] = json!([id]);
        let r2 = add_feature(&mut doc, &draft.to_string()).unwrap();
        assert!(r2["bodies"][0]["volume_mm3"].as_f64().unwrap() < 12000.0);
        assert_eq!(doc.features.len(), 2);
        // a feature that cannot build is rejected and the document stays as it was
        let mut bad = draft.clone();
        bad["Draft"]["bodies"] = json!([9999]);
        assert!(add_feature(&mut doc, &bad.to_string()).is_err());
        assert_eq!(doc.features.len(), 2);
        assert!(add_feature(&mut doc, "{not json").is_err());
    }

    #[test]
    fn report_lists_errors_overlaps_and_bounds() {
        let mut doc = Document::default();
        let b = catalog()["examples"]["Primitive/Box"].to_string();
        add_feature(&mut doc, &b).unwrap();
        add_feature(&mut doc, &b).unwrap();
        let r = report(&doc);
        assert_eq!(r["interferences"].as_array().unwrap().len(), 1);
        assert_eq!(r["bodies"][0]["bounds"]["max"], json!([40.0, 30.0, 10.0]));
    }
}
