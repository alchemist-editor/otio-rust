//! ORI's coordinates, checked with ORI's own test numbers and its test chart.

mod common;

use otio_core::{Box2d, V2d};
use otio_ext_ori_annotations::annotations;
use otio_ext_ori_annotations::command::Command;
use otio_ext_ori_annotations::coords::{self, ImageSize, aspect_half};

const HD: ImageSize = ImageSize::new(1920.0, 1080.0);

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// Ported from ORI's tests/otio_sync/test_coords.py.

#[test]
fn aspect_half_16_9() {
    assert!(close(aspect_half(1920.0, 1080.0), 8.0 / 9.0));
}

#[test]
fn aspect_half_falls_back_without_a_height() {
    assert_eq!(aspect_half(1920.0, 0.0), coords::DEFAULT_ASPECT_HALF);
    assert_eq!(aspect_half(1920.0, f64::NAN), coords::DEFAULT_ASPECT_HALF);
}

#[test]
fn the_centre_pixel_is_the_origin() {
    let (x, y) = HD.pixels_to_ori(960.0, 540.0);
    assert!(close(x, 0.0) && close(y, 0.0));
}

#[test]
fn pixels_round_trip() {
    for (px, py) in [(0.0, 0.0), (100.0, 250.0), (1919.0, 1079.0), (960.0, 540.0)] {
        let (x, y) = HD.pixels_to_ori(px, py);
        let (back_x, back_y) = HD.ori_to_pixels(x, y);
        assert!(close(back_x, px) && close(back_y, py));
    }
}

#[test]
fn x_runs_to_half_the_aspect() {
    assert!(close(HD.pixels_to_ori(1920.0, 540.0).0, HD.aspect_half()));
    assert!(close(HD.pixels_to_ori(0.0, 540.0).0, -HD.aspect_half()));
}

#[test]
fn y_is_up() {
    assert!(HD.pixels_to_ori(960.0, 200.0).1 > 0.0);
}

/// The first stroke in ORI's test chart outlines a rectangle on the
/// 1920x1080 `vector_shapes.png`, corners at pixels (150, 300) and
/// (550, 700). Converting those pixels gives the chart's numbers bit for bit,
/// so the conversion is ORI's own, not merely close to it.
#[test]
fn matches_the_test_chart() {
    common::enable();
    let document =
        otio_core::from_str(&common::fixture("testchart_annotations.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    let first = annotations::list(&document, timeline)
        .expect("lists")
        .remove(0);
    assert_eq!(
        first.target,
        annotations::Target::ClipName("vector_shapes.png".to_string())
    );
    let Command::PaintPoint(point) = Command::read(&document, first.commands[1]).expect("reads")
    else {
        panic!("not a point batch");
    };
    let points = point.points.expect("has points");
    let corners = [
        (150.0, 300.0),
        (550.0, 300.0),
        (550.0, 700.0),
        (150.0, 700.0),
        (150.0, 300.0),
    ];
    for (index, (px, py)) in corners.into_iter().enumerate() {
        assert_eq!(HD.pixels_to_ori(px, py), (points.x[index], points.y[index]));
        assert_eq!(HD.ori_to_pixels(points.x[index], points.y[index]), (px, py));
    }
}

#[test]
fn maps_onto_image_bounds() {
    // A 16:9 picture centred on (1, 2), 9 units high.
    let bounds = Box2d {
        min: V2d { x: -7.0, y: -2.5 },
        max: V2d { x: 9.0, y: 6.5 },
    };
    let point = coords::ori_to_bounds(0.5, -0.5, bounds);
    assert!(close(point.x, 5.5) && close(point.y, -2.5));
    let (x, y) = coords::bounds_to_ori(point, bounds);
    assert!(close(x, 0.5) && close(y, -0.5));
    assert!(close(
        coords::ori_to_bounds(8.0 / 9.0, 0.0, bounds).x,
        bounds.max.x
    ));
}
