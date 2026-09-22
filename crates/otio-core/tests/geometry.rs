//! `V2d` and `Box2d` arithmetic, checked against the values upstream's
//! `test_v2d.py` and `test_box2d.py` expect from Imath.

use otio_core::{Box2d, V2d};

#[test]
fn a_vector_has_imaths_products() {
    let a = V2d::new(1.0, 2.0);
    let b = V2d::new(3.0, 4.0);
    assert_eq!(a.dot(b), 11.0);
    assert_eq!(a.cross(b), -2.0);
    assert_eq!(a + b, V2d::new(4.0, 6.0));
    assert_eq!(a - b, V2d::new(-2.0, -2.0));
    assert_eq!(a * b, V2d::new(3.0, 8.0));
    assert_eq!(a / b, V2d::new(1.0 / 3.0, 0.5));
}

#[test]
fn a_vector_has_a_length_and_a_direction() {
    let v = V2d::new(3.0, 4.0);
    assert_eq!(v.length(), 5.0);
    assert_eq!(v.length2(), 25.0);
    assert_eq!(v.normalized(), V2d::new(0.6, 0.8));
    assert_eq!(v.normalized_checked(), Some(V2d::new(0.6, 0.8)));
    assert_eq!(v.normalized_unchecked(), V2d::new(0.6, 0.8));

    // A vector too short to square still has a length.
    assert_eq!(V2d::new(3e-200, 4e-200).length(), 5e-200);
}

#[test]
fn a_null_vector_normalizes_to_itself_or_not_at_all() {
    let null = V2d::default();
    assert_eq!(null.normalized(), null);
    assert_eq!(null.normalized_checked(), None);
    assert!(null.normalized_unchecked().x.is_nan());
}

#[test]
fn vectors_compare_within_an_error() {
    let a = V2d::new(1.0, 2.0);
    assert!(a.equal_with_abs_error(V2d::new(1.0, 2.0), 0.0));
    assert!(a.equal_with_abs_error(V2d::new(1.05, 2.0), 0.1));
    assert!(!a.equal_with_abs_error(V2d::new(1.2, 2.0), 0.1));
    assert!(a.equal_with_rel_error(V2d::new(1.0, 2.0), 0.0));
    assert!(a.equal_with_rel_error(V2d::new(1.0, 2.1), 0.1));
    assert!(!a.equal_with_rel_error(V2d::new(1.2, 2.0), 0.1));
}

#[test]
fn a_box_extends_to_hold_points_and_boxes() {
    let a = Box2d::new(V2d::new(1.0, 2.0), V2d::new(3.0, 4.0));
    assert_eq!(a.center(), V2d::new(2.0, 3.0));

    let a = a.extended_by_point(V2d::new(5.0, 5.0));
    assert_eq!(a, Box2d::new(V2d::new(1.0, 2.0), V2d::new(5.0, 5.0)));

    let a = a.extended_by(Box2d::new(V2d::new(2.0, 3.0), V2d::new(6.0, 6.0)));
    assert_eq!(a, Box2d::new(V2d::new(1.0, 2.0), V2d::new(6.0, 6.0)));
}

#[test]
fn a_box_intersects_what_it_touches() {
    let a = Box2d::new(V2d::new(1.0, 2.0), V2d::new(3.0, 4.0));
    assert!(a.contains_point(V2d::new(2.0, 3.0)));
    assert!(a.contains_point(V2d::new(1.0, 4.0)));
    assert!(!a.contains_point(V2d::new(0.0, 3.0)));

    assert!(a.intersects(Box2d::new(V2d::new(1.1, 1.9), V2d::new(3.1, 3.9))));
    assert!(a.intersects(Box2d::new(V2d::new(3.0, 4.0), V2d::new(5.0, 5.0))));
    assert!(!a.intersects(Box2d::new(V2d::new(3.1, 4.1), V2d::new(4.1, 5.1))));
}

#[test]
fn a_bare_value_reads_back_as_itself() {
    let json = r#"{"OTIO_SCHEMA": "V2d.1", "x": 0.3, "y": 0.4}"#;
    let (document, value) = otio_core::from_str_any(json).unwrap();
    assert_eq!(value, otio_core::Any::V2d(V2d::new(0.3, 0.4)));
    assert!(document.root().is_none());

    let json = r#"{"OTIO_SCHEMA": "Gap.1", "name": "g"}"#;
    let (document, value) = otio_core::from_str_any(json).unwrap();
    let otio_core::Any::Object(id) = value else {
        panic!("a gap is an object");
    };
    assert_eq!(document.root(), Some(id));
    assert_eq!(document.try_get(id).unwrap().name(), "g");
}
