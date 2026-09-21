// SPDX-License-Identifier: Apache-2.0
// Copyright Contributors to the OpenTimelineIO project

//! Reading and writing colours, including the two places where upstream's
//! conversions disagree with each other.

use otio_core::Color;

#[test]
fn a_named_colour_has_the_components_upstream_gives_it() {
    assert_eq!(Color::red().to_hex(), "#ff0000ff");
    assert_eq!(Color::green().to_hex(), "#00ff00ff");
    assert_eq!(Color::blue().to_hex(), "#0000ffff");
    assert_eq!(Color::transparent().to_hex(), "#00000000");
    assert_eq!(Color::orange().to_rgba_int_list(8), [255, 127, 0, 255]);
}

#[test]
fn pink_and_magenta_are_the_same_colour_under_two_names() {
    // Upstream defines both as (1, 0, 1, 1). Keeping them apart would be a
    // tidier palette and a different file format.
    assert_eq!(Color::pink().to_hex(), Color::magenta().to_hex());
    assert_eq!(Color::pink().name, "Pink");
    assert_eq!(Color::magenta().name, "Magenta");
}

#[test]
fn two_colours_are_the_same_when_they_look_the_same() {
    // The name is not part of the comparison, so a named red read out of an
    // old file equals the one this library hands out.
    assert!(Color::red().looks_like(&Color::rgb(1.0, 0.0, 0.0)));
    // Nor is a difference too small to survive the eight-bit form.
    assert!(Color::red().looks_like(&Color::rgb(1.0, 0.001, 0.0)));
    assert!(!Color::red().looks_like(&Color::green()));
}

#[test]
fn hex_is_read_in_all_four_lengths() {
    assert_eq!(Color::from_hex("#ff0000").unwrap().to_hex(), "#ff0000ff");
    assert_eq!(Color::from_hex("ff000080").unwrap().to_hex(), "#ff000080");
    assert_eq!(Color::from_hex("0xf00").unwrap().to_hex(), "#ff0000ff");
    assert_eq!(
        Color::from_hex("#f008").unwrap().to_rgba_int_list(8)[3],
        136
    );
}

#[test]
fn a_hex_string_of_the_wrong_length_is_refused() {
    assert!(Color::from_hex("#ff").is_err());
    assert!(Color::from_hex("#fffff").is_err());
    assert!(Color::from_hex("#gggggg").is_err());
}

#[test]
fn a_list_must_hold_three_or_four_components() {
    assert!(Color::from_float_list(&[1.0, 0.0]).is_err());
    assert!(Color::from_float_list(&[1.0, 0.0, 0.0, 1.0, 1.0]).is_err());
    assert_eq!(Color::from_float_list(&[1.0, 0.0, 0.0]).unwrap().a, 1.0);
    assert_eq!(
        Color::from_int_list(&[255, 0, 0], 8).unwrap().to_hex(),
        "#ff0000ff"
    );
}

#[test]
fn packing_and_unpacking_an_integer_swaps_green_and_blue() {
    // This is upstream's bug, kept deliberately: `to_agbr_integer` writes
    // blue at bits 16-23 and green at 8-15, and `from_agbr_int` reads them
    // the other way round. A file or a plugin that went through upstream
    // carries whatever upstream produced, so "fixing" it here would make this
    // library disagree with every other reader.
    let green = Color::rgb(0.0, 1.0, 0.0);
    let round_tripped = Color::from_agbr_int(green.to_agbr_integer());
    assert_eq!(round_tripped.to_hex(), "#0000ffff");

    // A colour with green and blue equal is unaffected, which is why the bug
    // survives: the greys and the primaries other than green and blue all
    // round trip.
    for colour in [Color::red(), Color::black(), Color::white()] {
        let round_tripped = Color::from_agbr_int(colour.to_agbr_integer());
        assert_eq!(round_tripped.to_hex(), colour.to_hex(), "{}", colour.name);
    }
}
