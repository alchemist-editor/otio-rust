//! Reading errors, in upstream's words.
//!
//! Each expected message is what OpenTimelineIO itself raises for the same
//! input: its Python bindings' `ValueError` text, with the C++ type names
//! spelled for 0.19's `v0_19` namespace (the messages were taken from 0.18.1,
//! whose namespace is `v0_18_1`). Code in the wild matches on this text, so
//! it is pinned character for character.

/// A timeline upstream wrote, which each case below breaks in one place.
const BASE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "metadata": {},
    "name": "tl",
    "global_start_time": null,
    "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "metadata": {},
        "name": "tracks",
        "source_range": null,
        "effects": [],
        "markers": [],
        "enabled": true,
        "color": null,
        "children": [
            {
                "OTIO_SCHEMA": "Track.1",
                "metadata": {},
                "name": "V1",
                "source_range": null,
                "effects": [],
                "markers": [],
                "enabled": true,
                "color": null,
                "children": [
                    {
                        "OTIO_SCHEMA": "Clip.2",
                        "metadata": {},
                        "name": "shot",
                        "source_range": {
                            "OTIO_SCHEMA": "TimeRange.1",
                            "duration": {
                                "OTIO_SCHEMA": "RationalTime.1",
                                "rate": 24.0,
                                "value": 10.0
                            },
                            "start_time": {
                                "OTIO_SCHEMA": "RationalTime.1",
                                "rate": 24.0,
                                "value": 0.0
                            }
                        },
                        "effects": [
                            {
                                "OTIO_SCHEMA": "LinearTimeWarp.1",
                                "metadata": {},
                                "name": "lt",
                                "effect_name": "LinearTimeWarp",
                                "enabled": true,
                                "time_scalar": 2.0
                            }
                        ],
                        "markers": [
                            {
                                "OTIO_SCHEMA": "Marker.2",
                                "metadata": {},
                                "name": "m",
                                "color": "RED",
                                "marked_range": {
                                    "OTIO_SCHEMA": "TimeRange.1",
                                    "duration": {
                                        "OTIO_SCHEMA": "RationalTime.1",
                                        "rate": 24.0,
                                        "value": 0.0
                                    },
                                    "start_time": {
                                        "OTIO_SCHEMA": "RationalTime.1",
                                        "rate": 24.0,
                                        "value": 1.0
                                    }
                                },
                                "comment": ""
                            }
                        ],
                        "enabled": true,
                        "color": null,
                        "media_references": {
                            "DEFAULT_MEDIA": {
                                "OTIO_SCHEMA": "ExternalReference.1",
                                "metadata": {},
                                "name": "",
                                "available_range": {
                                    "OTIO_SCHEMA": "TimeRange.1",
                                    "duration": {
                                        "OTIO_SCHEMA": "RationalTime.1",
                                        "rate": 24.0,
                                        "value": 50.0
                                    },
                                    "start_time": {
                                        "OTIO_SCHEMA": "RationalTime.1",
                                        "rate": 24.0,
                                        "value": 0.0
                                    }
                                },
                                "available_image_bounds": null,
                                "target_url": "a.mov"
                            }
                        },
                        "active_media_reference_key": "DEFAULT_MEDIA"
                    },
                    {
                        "OTIO_SCHEMA": "Gap.1",
                        "metadata": {},
                        "name": "g",
                        "source_range": {
                            "OTIO_SCHEMA": "TimeRange.1",
                            "duration": {
                                "OTIO_SCHEMA": "RationalTime.1",
                                "rate": 24.0,
                                "value": 5.0
                            },
                            "start_time": {
                                "OTIO_SCHEMA": "RationalTime.1",
                                "rate": 24.0,
                                "value": 0.0
                            }
                        },
                        "effects": [],
                        "markers": [],
                        "enabled": true,
                        "color": null
                    },
                    {
                        "OTIO_SCHEMA": "Transition.1",
                        "metadata": {},
                        "name": "tx",
                        "in_offset": {
                            "OTIO_SCHEMA": "RationalTime.1",
                            "rate": 24.0,
                            "value": 1.0
                        },
                        "out_offset": {
                            "OTIO_SCHEMA": "RationalTime.1",
                            "rate": 24.0,
                            "value": 1.0
                        },
                        "transition_type": ""
                    },
                    {
                        "OTIO_SCHEMA": "Clip.2",
                        "metadata": {},
                        "name": "shot2",
                        "source_range": {
                            "OTIO_SCHEMA": "TimeRange.1",
                            "duration": {
                                "OTIO_SCHEMA": "RationalTime.1",
                                "rate": 24.0,
                                "value": 10.0
                            },
                            "start_time": {
                                "OTIO_SCHEMA": "RationalTime.1",
                                "rate": 24.0,
                                "value": 0.0
                            }
                        },
                        "effects": [],
                        "markers": [],
                        "enabled": true,
                        "color": null,
                        "media_references": {
                            "DEFAULT_MEDIA": {
                                "OTIO_SCHEMA": "ImageSequenceReference.1",
                                "metadata": {},
                                "name": "",
                                "available_range": null,
                                "available_image_bounds": null,
                                "target_url_base": "/x/",
                                "name_prefix": "a.",
                                "name_suffix": ".exr",
                                "start_frame": 1,
                                "frame_step": 1,
                                "rate": 24.0,
                                "frame_zero_padding": 0,
                                "missing_frame_policy": "error"
                            }
                        },
                        "active_media_reference_key": "DEFAULT_MEDIA"
                    }
                ],
                "kind": "Video"
            }
        ]
    }
}"#;

/// Replaces lines `first` to `last` of [`BASE`], counted from one, with
/// `text`.
fn broken(first: usize, last: usize, text: &str) -> String {
    let lines: Vec<&str> = BASE.lines().collect();
    let mut result: Vec<&str> = lines[..first - 1].to_vec();
    result.push(text);
    result.extend(&lines[last..]);
    result.join("\n")
}

/// Reads `text`, and gives the error's message.
fn message(text: &str) -> String {
    match otio_core::from_str(text) {
        Ok(_) => "read without error".to_string(),
        Err(error) => error.to_string(),
    }
}

#[test]
fn the_base_document_reads() {
    otio_core::from_str(BASE).unwrap();
}

#[test]
fn decoding_errors_are_upstreams() {
    // (what is wrong, first line replaced, last line replaced, replacement,
    // upstream's message)
    let cases: &[(&str, usize, usize, &str, &str)] = &[
        (
            "a clip's name is a number",
            29,
            29,
            "\"name\": 5,",
            "type mismatch while decoding: While reading object named '<unknown>' (of type 'N14opentimelineio5v0_194ClipE'): expected type string under key 'name': found type l instead (near line 100)",
        ),
        (
            "a clip's name is a list",
            29,
            29,
            "\"name\": [],",
            "type mismatch while decoding: While reading object named '<unknown>' (of type 'N14opentimelineio5v0_194ClipE'): expected type string under key 'name': found type N14opentimelineio5v0_199AnyVectorE instead (near line 100)",
        ),
        (
            "a clip's enabled flag is a number",
            75,
            75,
            "\"enabled\": 3,",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type b under key 'enabled': found type l instead (near line 100)",
        ),
        (
            "a clip's metadata is a number",
            28,
            28,
            "\"metadata\": 3,",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type N14opentimelineio5v0_1913AnyDictionaryE under key 'metadata': found type l instead (near line 100)",
        ),
        (
            "a clip's metadata is an object",
            28,
            28,
            "\"metadata\": {\"OTIO_SCHEMA\": \"Gap.1\", \"name\": \"gg\", \"source_range\": null, \"effects\": [], \"markers\": [], \"enabled\": true, \"metadata\": {}},",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type N14opentimelineio5v0_1913AnyDictionaryE under key 'metadata': found type N14opentimelineio5v0_1918SerializableObject8RetainerIS1_EE instead (near line 100)",
        ),
        (
            "a clip's colour is a number",
            76,
            76,
            "\"color\": 3,",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type N14opentimelineio5v0_195ColorE under key 'color': found type l instead (near line 100)",
        ),
        (
            "a clip's source range is a number",
            30,
            42,
            "\"source_range\": 3,",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type N8opentime5v0_199TimeRangeE under key 'source_range': found type l instead (near line 88)",
        ),
        (
            "a clip's source range has no schema",
            30,
            42,
            "\"source_range\": {\"a\": 1},",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type N8opentime5v0_199TimeRangeE under key 'source_range': found type N14opentimelineio5v0_1913AnyDictionaryE instead (near line 88)",
        ),
        (
            "a clip's source range is a time",
            30,
            42,
            "\"source_range\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"rate\": 1, \"value\": 1},",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected type N8opentime5v0_199TimeRangeE under key 'source_range': found type N8opentime5v0_1912RationalTimeE instead (near line 88)",
        ),
        (
            "a clip's markers are a number",
            53,
            74,
            "\"markers\": 3,",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): while decoding complex STL type, expected type 'N14opentimelineio5v0_199AnyVectorE', found type 'l' instead (near line 79)",
        ),
        (
            "a clip's marker is a number",
            53,
            74,
            "\"markers\": [3],",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected to read a N14opentimelineio5v0_196MarkerE, found a l instead (near line 79)",
        ),
        (
            "a clip's marker has no schema",
            53,
            74,
            "\"markers\": [{\"a\": 1}],",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected to read a N14opentimelineio5v0_196MarkerE, found a N14opentimelineio5v0_1913AnyDictionaryE instead (near line 79)",
        ),
        (
            "a clip's marker is a gap",
            53,
            74,
            "\"markers\": [{\"OTIO_SCHEMA\": \"Gap.1\", \"name\": \"gg\", \"source_range\": null, \"effects\": [], \"markers\": [], \"enabled\": true, \"metadata\": {}}],",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected to read a N14opentimelineio5v0_196MarkerE, found a N14opentimelineio5v0_193GapE instead (near line 79)",
        ),
        (
            "a clip's media references are a number",
            77,
            98,
            "\"media_references\": 3,",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): while decoding complex STL type, expected type 'N14opentimelineio5v0_1913AnyDictionaryE', found type 'l' instead (near line 79)",
        ),
        (
            "a clip's media reference is a number",
            77,
            98,
            "\"media_references\": {\"DEFAULT_MEDIA\": 3},",
            "type mismatch while decoding: While reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): expected to read a N14opentimelineio5v0_1914MediaReferenceE, found a l instead (near line 79)",
        ),
        (
            "a track's children are a number",
            25,
            179,
            "\"children\": 3,",
            "type mismatch while decoding: While reading object named 'V1' (of type 'N14opentimelineio5v0_195TrackE'): while decoding complex STL type, expected type 'N14opentimelineio5v0_199AnyVectorE', found type 'l' instead (near line 27)",
        ),
        (
            "a track's child is a marker",
            25,
            179,
            "\"children\": [{\"OTIO_SCHEMA\": \"Marker.2\", \"name\": \"mm\", \"marked_range\": null, \"color\": \"RED\", \"metadata\": {}, \"comment\": \"\"}],",
            "type mismatch while decoding: While reading object named 'V1' (of type 'N14opentimelineio5v0_195TrackE'): expected to read a N14opentimelineio5v0_1910ComposableE, found a N14opentimelineio5v0_196MarkerE instead (near line 27)",
        ),
        (
            "a track's child is an unresolved reference",
            25,
            179,
            "\"children\": [{\"OTIO_SCHEMA\": \"SerializableObjectRef.1\", \"id\": \"nope\"}],",
            "type mismatch while decoding: While reading object named 'V1' (of type 'N14opentimelineio5v0_195TrackE'): expected to read a N14opentimelineio5v0_1910ComposableE, found a N14opentimelineio5v0_1918SerializableObject11ReferenceIdE instead (near line 27)",
        ),
        (
            "a track's kind is a number",
            180,
            180,
            "\"kind\": 3",
            "type mismatch while decoding: While reading object named 'V1' (of type 'N14opentimelineio5v0_195TrackE'): expected type string under key 'kind': found type l instead (near line 181)",
        ),
        (
            "a timeline's tracks are a number",
            6,
            183,
            "\"tracks\": 3",
            "type mismatch while decoding: While reading object named 'tl' (of type 'N14opentimelineio5v0_198TimelineE'): expected to read a N14opentimelineio5v0_1918SerializableObjectE, found a l instead (near line 7)",
        ),
        (
            "a timeline's tracks are a list",
            6,
            183,
            "\"tracks\": []",
            "type mismatch while decoding: While reading object named 'tl' (of type 'N14opentimelineio5v0_198TimelineE'): expected to read a N14opentimelineio5v0_1918SerializableObjectE, found a N14opentimelineio5v0_199AnyVectorE instead (near line 7)",
        ),
        (
            "a timeline's tracks are a gap",
            6,
            183,
            "\"tracks\": {\"OTIO_SCHEMA\": \"Gap.1\", \"name\": \"gg\", \"source_range\": null, \"effects\": [], \"markers\": [], \"enabled\": true, \"metadata\": {}}",
            "type mismatch while decoding: While reading object named 'tl' (of type 'N14opentimelineio5v0_198TimelineE'): Expected object of type N14opentimelineio5v0_195StackE; read type N14opentimelineio5v0_193GapE instead (near line 7)",
        ),
        (
            "a timeline's start time is a number",
            5,
            5,
            "\"global_start_time\": 3,",
            "type mismatch while decoding: While reading object named 'tl' (of type 'N14opentimelineio5v0_198TimelineE'): expected type N8opentime5v0_1912RationalTimeE under key 'global_start_time': found type l instead (near line 184)",
        ),
        (
            "a time's rate is a string",
            39,
            39,
            "\"rate\": \"x\",",
            "type mismatch while decoding: near line 41",
        ),
        (
            "a range's start time is a number",
            37,
            41,
            "\"start_time\": 3",
            "type mismatch while decoding: near line 38",
        ),
        (
            "a transition's offset is a number",
            127,
            131,
            "\"in_offset\": 3,",
            "type mismatch while decoding: While reading object named 'tx' (of type 'N14opentimelineio5v0_1910TransitionE'): expected type N8opentime5v0_1912RationalTimeE under key 'in_offset': found type l instead (near line 134)",
        ),
        (
            "a time warp's scalar is a string",
            50,
            50,
            "\"time_scalar\": \"x\"",
            "type mismatch while decoding: While reading object named 'lt' (of type 'N14opentimelineio5v0_1914LinearTimeWarpE'): expected type d under key 'time_scalar': found type string instead (near line 51)",
        ),
        (
            "a reference's bounds are a number",
            95,
            95,
            "\"available_image_bounds\": 3,",
            "type mismatch while decoding: While reading object named '' (of type 'N14opentimelineio5v0_1917ExternalReferenceE'): expected type N9Imath_3_23BoxINS_4Vec2IdEEEE under key 'available_image_bounds': found type l instead (near line 97)",
        ),
        (
            "a sequence's start frame is fractional",
            170,
            170,
            "\"start_frame\": 1.5,",
            "type mismatch while decoding: While reading object named '' (of type 'N14opentimelineio5v0_1922ImageSequenceReferenceE'): while decoding complex STL type, expected type 'l', found type 'd' instead (near line 175)",
        ),
        (
            "a sequence's policy is unknown",
            174,
            174,
            "\"missing_frame_policy\": \"zzz\"",
            "JSON parse error while reading: While reading object named '' (of type 'N14opentimelineio5v0_1922ImageSequenceReferenceE'): Unknown missing_frame_policy: zzz (near line 175)",
        ),
        (
            "a sequence's policy is a number",
            174,
            174,
            "\"missing_frame_policy\": 3",
            "JSON parse error while reading: While reading object named '' (of type 'N14opentimelineio5v0_1922ImageSequenceReferenceE'): Unknown missing_frame_policy:  (near line 175)",
        ),
        (
            "a schema has no version",
            27,
            27,
            "\"OTIO_SCHEMA\": \"Clip\",",
            "Illegal/malformed schema: near line 100",
        ),
        (
            "a schema's version does not fit",
            27,
            27,
            "\"OTIO_SCHEMA\": \"Clip.99999999999\",",
            "Illegal/malformed schema: near line 100",
        ),
        (
            "a schema is a number",
            27,
            27,
            "\"OTIO_SCHEMA\": 3,",
            "type mismatch while decoding: near line 100",
        ),
        (
            "a reference is unresolved",
            28,
            28,
            "\"metadata\": {\"r\": {\"OTIO_SCHEMA\": \"SerializableObjectRef.1\", \"id\": \"nope\"}},",
            "Unresolved object reference while reading: nope (near line 100)",
        ),
        (
            "a reference deep in metadata is unresolved",
            28,
            28,
            "\"metadata\": {\"r\": [\n{\"x\": {\"OTIO_SCHEMA\": \"SerializableObjectRef.1\", \"id\": \"nope\"}}\n]},",
            "Unresolved object reference while reading: nope (near line 102)",
        ),
        (
            "a reference's id is a number",
            28,
            28,
            "\"metadata\": {\"r\": {\"OTIO_SCHEMA\": \"SerializableObjectRef.1\", \"id\": 3}},",
            "type mismatch while decoding: near line 28",
        ),
        (
            "a colour's component is a string",
            58,
            58,
            "\"color\": {\"OTIO_SCHEMA\": \"Color.1\", \"r\": \"x\", \"g\": 0, \"b\": 0, \"a\": 1, \"name\": \"\"},",
            "type mismatch while decoding: near line 58",
        ),
    ];
    let mut failures = Vec::new();
    for (label, first, last, text, expected) in cases {
        let message = message(&broken(*first, *last, text));
        if message != *expected {
            failures.push(format!(
                "{label}:\n  expected: {expected}\n  got:      {message}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn json_syntax_errors_are_rapidjsons() {
    // Upstream parses with RapidJSON and reports its message and position:
    // the line, and how many bytes into it the parser had read.
    let cases: &[(&str, &str)] = &[
        (
            "",
            "JSON parse error while reading: JSON parse error on input string: The document is empty. (line 1, column 0)",
        ),
        (
            "{",
            "JSON parse error while reading: JSON parse error on input string: Missing a name for object member. (line 1, column 1)",
        ),
        (
            "[",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 1)",
        ),
        (
            "{\"a\"",
            "JSON parse error while reading: JSON parse error on input string: Missing a colon after a name of object member. (line 1, column 4)",
        ),
        (
            "{\"a\":",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 5)",
        ),
        (
            "{\"a\": 1",
            "JSON parse error while reading: JSON parse error on input string: Missing a comma or '}' after an object member. (line 1, column 7)",
        ),
        (
            "{\"a\": 1,",
            "JSON parse error while reading: JSON parse error on input string: Missing a name for object member. (line 1, column 8)",
        ),
        (
            "[1,",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 3)",
        ),
        (
            "[1 2]",
            "JSON parse error while reading: JSON parse error on input string: Missing a comma or ']' after an array element. (line 1, column 3)",
        ),
        (
            "{\"a\" 1}",
            "JSON parse error while reading: JSON parse error on input string: Missing a colon after a name of object member. (line 1, column 5)",
        ),
        (
            "{1: 2}",
            "JSON parse error while reading: JSON parse error on input string: Missing a name for object member. (line 1, column 1)",
        ),
        (
            "tru",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 3)",
        ),
        (
            "nul",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 3)",
        ),
        (
            "\"abc",
            "JSON parse error while reading: JSON parse error on input string: Missing a closing quotation mark in string. (line 1, column 4)",
        ),
        (
            "\"\\x\"",
            "JSON parse error while reading: JSON parse error on input string: Invalid escape character in string. (line 1, column 2)",
        ),
        (
            "\"\\u12G4\"",
            "JSON parse error while reading: JSON parse error on input string: Incorrect hex digit after \\u escape in string. (line 1, column 5)",
        ),
        (
            "\"\\uD800\"",
            "JSON parse error while reading: JSON parse error on input string: The surrogate pair in string is invalid. (line 1, column 7)",
        ),
        (
            "\"\\uD800\\u0041\"",
            "JSON parse error while reading: JSON parse error on input string: The surrogate pair in string is invalid. (line 1, column 13)",
        ),
        (
            "1.",
            "JSON parse error while reading: JSON parse error on input string: Miss fraction part in number. (line 1, column 2)",
        ),
        (
            "1e",
            "JSON parse error while reading: JSON parse error on input string: Miss exponent in number. (line 1, column 2)",
        ),
        (
            "-",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 1)",
        ),
        (
            "01",
            "JSON parse error while reading: JSON parse error on input string: The document root must not be followed by other values. (line 1, column 1)",
        ),
        (
            "{}\n{}",
            "JSON parse error while reading: JSON parse error on input string: The document root must not be followed by other values. (line 2, column 0)",
        ),
        (
            "{} x",
            "JSON parse error while reading: JSON parse error on input string: The document root must not be followed by other values. (line 1, column 3)",
        ),
        (
            "{\n  \"a\": 1\n  \"b\": 2\n}",
            "JSON parse error while reading: JSON parse error on input string: Missing a comma or '}' after an object member. (line 3, column 2)",
        ),
        (
            "[\n1,\n2\n,]",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 4, column 1)",
        ),
        (
            "@",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 0)",
        ),
        (
            "{\"a\":1}}",
            "JSON parse error while reading: JSON parse error on input string: The document root must not be followed by other values. (line 1, column 7)",
        ),
        (
            "1e999",
            "JSON parse error while reading: JSON parse error on input string: Number too big to be stored in double. (line 1, column 5)",
        ),
        (
            "{\"a\":\t@}",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 6)",
        ),
        (
            "\n\n  {\"x\": [1, 2,, 3]}",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 3, column 14)",
        ),
        (
            "Nan",
            "JSON parse error while reading: JSON parse error on input string: Invalid value. (line 1, column 2)",
        ),
        (
            "\"a\u{0001}b\"",
            "JSON parse error while reading: JSON parse error on input string: Invalid encoding in string. (line 1, column 2)",
        ),
    ];
    let mut failures = Vec::new();
    for (text, expected) in cases {
        let message = message(text);
        if message != *expected {
            failures.push(format!(
                "{text:?}:\n  expected: {expected}\n  got:      {message}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_reference_id_declared_twice_is_refused() {
    // A `SerializableObjectRef` names the object it points at by its
    // `OTIO_REF_ID`, so two objects declaring one id would leave a reference
    // meaning either. Upstream's reader refuses the document with
    // DUPLICATE_OBJECT_REFERENCE (a `ValueError`) rather than let the later
    // object win. It checks an object's id as soon as it reaches the object's
    // closing brace, having already decoded everything inside it, and before
    // it looks at the schema string: so the object reported is the second to
    // be closed, which for a parent and child is the parent, and a duplicate
    // outranks a malformed or too-new schema on the same object. Its message
    // then gives that line and drops the id. An empty id declares nothing,
    // and the value types (`TimeRange`, `RationalTime` and the rest) are not
    // objects and declare nothing either. An id that is not a string is a
    // type mismatch, again by line alone.
    //
    // (what is wrong, the document, upstream's message)
    let cases: &[(&str, &str, &str)] = &[
        (
            "two siblings declare the same id",
            r#"{
    "OTIO_SCHEMA": "Track.1",
    "kind": "Video",
    "children": [
        {
            "OTIO_SCHEMA": "Gap.1",
            "OTIO_REF_ID": "Gap-1"
        },
        {
            "OTIO_SCHEMA": "Gap.1",
            "OTIO_REF_ID": "Gap-1"
        }
    ]
}"#,
            "Duplicated object reference while reading: near line 12",
        ),
        (
            "an object declares its parent's id",
            r#"{
    "OTIO_SCHEMA": "Stack.1",
    "OTIO_REF_ID": "shared",
    "children": [
        {
            "OTIO_SCHEMA": "Gap.1",
            "OTIO_REF_ID": "shared"
        }
    ]
}"#,
            "Duplicated object reference while reading: near line 10",
        ),
        (
            "two objects in metadata declare the same id",
            r#"{
    "OTIO_SCHEMA": "Gap.1",
    "metadata": {
        "a": {"OTIO_SCHEMA": "Marker.2", "OTIO_REF_ID": "m"},
        "b": {
            "OTIO_SCHEMA": "Marker.2",
            "OTIO_REF_ID": "m"
        }
    }
}"#,
            "Duplicated object reference while reading: near line 8",
        ),
        (
            "the duplicate also has a malformed schema",
            r#"{
    "OTIO_SCHEMA": "Stack.1",
    "children": [
        {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": "g"},
        {"OTIO_SCHEMA": "Gap", "OTIO_REF_ID": "g"}
    ]
}"#,
            "Duplicated object reference while reading: near line 5",
        ),
        (
            "the duplicate also has a schema too new",
            r#"{
    "OTIO_SCHEMA": "Stack.1",
    "children": [
        {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": "g"},
        {"OTIO_SCHEMA": "Gap.9", "OTIO_REF_ID": "g"}
    ]
}"#,
            "Duplicated object reference while reading: near line 5",
        ),
        (
            "the duplicate is of an unknown schema",
            r#"{
    "OTIO_SCHEMA": "Gap.1",
    "OTIO_REF_ID": "g",
    "metadata": {
        "x": {"OTIO_SCHEMA": "NoSuchThing.1", "OTIO_REF_ID": "g"}
    }
}"#,
            "Duplicated object reference while reading: near line 7",
        ),
        (
            "the duplicate is where a stack must be",
            r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "OTIO_REF_ID": "t",
    "metadata": {"x": {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": "g"}},
    "tracks": {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": "g"}
}"#,
            "Duplicated object reference while reading: near line 5",
        ),
        (
            "an id is a number",
            "{\n    \"OTIO_SCHEMA\": \"Gap.1\",\n    \"OTIO_REF_ID\": 3\n}",
            "type mismatch while decoding: near line 4",
        ),
        (
            "an id is null",
            "{\n    \"OTIO_SCHEMA\": \"Gap.1\",\n    \"OTIO_REF_ID\": null\n}",
            "type mismatch while decoding: near line 4",
        ),
        (
            "two objects have an empty id",
            r#"{
    "OTIO_SCHEMA": "Stack.1",
    "children": [
        {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": ""},
        {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": ""}
    ]
}"#,
            "read without error",
        ),
        (
            "value types carry an object's id",
            r#"{
    "OTIO_SCHEMA": "Gap.1",
    "OTIO_REF_ID": "g",
    "source_range": {
        "OTIO_SCHEMA": "TimeRange.1",
        "OTIO_REF_ID": "g",
        "start_time": {"OTIO_SCHEMA": "RationalTime.1", "OTIO_REF_ID": "g", "value": 0, "rate": 24},
        "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 1, "rate": 24}
    }
}"#,
            "read without error",
        ),
        (
            "distinct ids, one referred to",
            r#"{
    "OTIO_SCHEMA": "Stack.1",
    "children": [
        {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": "a", "name": "first"},
        {"OTIO_SCHEMA": "Gap.1", "OTIO_REF_ID": "b",
         "metadata": {"r": {"OTIO_SCHEMA": "SerializableObjectRef.1", "id": "a"}}}
    ]
}"#,
            "read without error",
        ),
    ];
    let mut failures = Vec::new();
    for (label, text, expected) in cases {
        let message = message(text);
        if message != *expected {
            failures.push(format!(
                "{label}:\n  expected: {expected}\n  got:      {message}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
