// SPDX-License-Identifier: Apache-2.0
// Copyright Contributors to the OpenTimelineIO project

//! Frame numbers and file names for an image sequence reference.

use opentime::{RationalTime, TimeRange};

use otio_core::schema::{ImageSequenceReference, MediaReferenceData, MissingFramePolicy};

/// The sequence upstream's own tests use: 60 frames at 30fps, every third
/// frame rendered, names like `show_shot.00001.exr`.
fn sequence() -> ImageSequenceReference {
    ImageSequenceReference {
        media: MediaReferenceData {
            available_range: Some(TimeRange::new(
                RationalTime::new(0.0, 30.0),
                RationalTime::new(60.0, 30.0),
            )),
            ..MediaReferenceData::default()
        },
        target_url_base: "file:///show/seq/shot/rndr/".to_string(),
        name_prefix: "show_shot.".to_string(),
        name_suffix: ".exr".to_string(),
        start_frame: 1,
        frame_step: 3,
        rate: 30.0,
        frame_zero_padding: 5,
        missing_frame_policy: MissingFramePolicy::Error,
    }
}

#[test]
fn a_frame_step_makes_fewer_images_than_frames() {
    let sequence = sequence();
    assert_eq!(sequence.number_of_images_in_sequence(), 20);
    assert_eq!(sequence.end_frame(), 60);
}

#[test]
fn a_sequence_with_no_range_is_one_frame_long() {
    let mut sequence = sequence();
    sequence.media.available_range = None;
    assert_eq!(sequence.number_of_images_in_sequence(), 0);
    assert_eq!(sequence.end_frame(), sequence.start_frame);
}

#[test]
fn the_url_carries_the_frame_number_padded() {
    let sequence = sequence();
    assert_eq!(
        sequence.target_url_for_image_number(0).unwrap(),
        "file:///show/seq/shot/rndr/show_shot.00001.exr"
    );
    assert_eq!(
        sequence.target_url_for_image_number(1).unwrap(),
        "file:///show/seq/shot/rndr/show_shot.00004.exr"
    );
}

#[test]
fn a_negative_frame_number_keeps_its_sign_outside_the_padding() {
    let mut sequence = sequence();
    sequence.start_frame = -1;
    sequence.frame_step = 1;
    sequence.frame_zero_padding = 4;
    assert_eq!(
        sequence.target_url_for_image_number(0).unwrap(),
        "file:///show/seq/shot/rndr/show_shot.-0001.exr"
    );
    assert_eq!(
        sequence.target_url_for_image_number(1).unwrap(),
        "file:///show/seq/shot/rndr/show_shot.0000.exr"
    );
}

#[test]
fn a_base_with_no_trailing_slash_gets_one() {
    let mut sequence = sequence();
    sequence.target_url_base = "file:///show/seq/shot/rndr".to_string();
    assert!(
        sequence
            .target_url_for_image_number(0)
            .unwrap()
            .starts_with("file:///show/seq/shot/rndr/show_shot.")
    );
}

#[test]
fn a_sequence_holding_no_images_says_so_rather_than_naming_a_file() {
    let mut sequence = sequence();
    sequence.media.available_range = None;
    // Upstream's wording, because its own tests compare the message.
    assert_eq!(
        sequence
            .target_url_for_image_number(0)
            .unwrap_err()
            .to_string(),
        "Zero duration sequences has no frames."
    );

    let mut sequence = sequence.clone();
    sequence.media.available_range = Some(TimeRange::new(
        RationalTime::new(0.0, 30.0),
        RationalTime::new(60.0, 30.0),
    ));
    sequence.rate = 0.0;
    assert_eq!(
        sequence
            .target_url_for_image_number(0)
            .unwrap_err()
            .to_string(),
        "Zero rate sequence has no frames."
    );
}

#[test]
fn asking_past_the_last_image_is_an_error() {
    let sequence = sequence();
    assert!(sequence.target_url_for_image_number(20).is_err());
    assert!(sequence.presentation_time_for_image_number(20).is_err());
}

#[test]
fn a_frame_number_can_be_found_from_a_time_inside_the_range() {
    let sequence = sequence();
    assert_eq!(
        sequence
            .frame_for_time(RationalTime::new(0.0, 30.0))
            .unwrap(),
        1
    );
    assert_eq!(
        sequence
            .frame_for_time(RationalTime::new(30.0, 30.0))
            .unwrap(),
        31
    );
    assert!(
        sequence
            .frame_for_time(RationalTime::new(90.0, 30.0))
            .is_err()
    );
}

#[test]
fn each_image_is_shown_a_frame_step_after_the_last() {
    let sequence = sequence();
    assert_eq!(
        sequence.presentation_time_for_image_number(0).unwrap(),
        RationalTime::new(0.0, 30.0)
    );
    assert_eq!(
        sequence.presentation_time_for_image_number(1).unwrap(),
        RationalTime::new(3.0, 30.0)
    );
}

#[test]
fn a_policy_name_this_library_does_not_know_is_refused() {
    // Upstream refuses the file rather than falling back to `error`: an
    // unrecognized policy would change what a player does with the media.
    let json = r#"{
        "OTIO_SCHEMA": "ImageSequenceReference.1",
        "target_url_base": "file:///show/",
        "missing_frame_policy": "BOGUS"
    }"#;
    assert!(otio_core::from_str(json).is_err());

    let json = json.replace("BOGUS", "hold");
    let document = otio_core::from_str(&json).unwrap();
    let root = document.root().unwrap();
    match document.try_get(root).unwrap() {
        otio_core::Node::ImageSequenceReference(sequence) => {
            assert_eq!(sequence.missing_frame_policy, MissingFramePolicy::Hold);
        }
        node => panic!("expected an image sequence, got a {}", node.schema_name()),
    }
}
