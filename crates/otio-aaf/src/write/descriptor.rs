//! The essence descriptors on file source mobs: upstream's two
//! `default_descriptor`s and `transcribe_otio_aaf_descriptor`.
//!
//! A file mob says what its media is: a picture of some size and layout, or
//! sound at some rate. Upstream builds the descriptor from what the reader
//! kept under `metadata["AAF"]["EssenceDescription"]`, fills in defaults
//! for what is missing, and then copies across every other property the
//! metadata names that the descriptor's class has. The order the properties
//! are set in is part of the file, so it is kept exactly, including the
//! order the metadata dictionary gives its keys in.

use aaf::Auid;
use aaf::write::{ObjRef, WriteValue};
use opentime::RationalTime;
use otio_core::{Any, Node, NodeId};

use super::py::{self, PyDict, aaf_metadata};
use super::{Fail, FileTranscriber, media_reference, metadata_of, unwritable};

/// `metadata.get("AAF", {}).get("EssenceDescription", {})` on a clip's
/// media.
fn essence_description(t: &FileTranscriber<'_>, clip: NodeId) -> Result<PyDict, Fail> {
    let media = media_reference(t.document, clip)?;
    aaf_metadata(metadata_of(t.document, media)?)?.get_dict("EssenceDescription")
}

/// `descriptor_dict.get(key, default)`.
fn get_or(dict: &PyDict, key: &str, default: Any) -> Any {
    dict.get(key).cloned().unwrap_or(default)
}

impl FileTranscriber<'_> {
    /// `VideoTrackTranscriber.default_descriptor`: a picture descriptor, of
    /// the class the metadata names or a CDCI descriptor, sized 1920 by 1080
    /// at 16:9 unless the metadata says otherwise.
    pub(crate) fn video_descriptor(&mut self, clip: NodeId) -> Result<ObjRef, Fail> {
        let dict = essence_description(self, clip)?;

        let descriptor = match dict.get("ClassName") {
            Some(class) if py::truthy(class) => {
                let Any::String(class) = class else {
                    return Err(unwritable(format!(
                        "an essence description whose ClassName is a {}",
                        class.type_name()
                    )));
                };
                self.f.create(class)?
            }
            _ => self.f.create("CDCIDescriptor")?,
        };

        let linemap = py::py_iter(&get_or(
            &dict,
            "VideoLineMap",
            Any::Vector(vec![Any::Int(42), Any::Int(0)]),
        ))?
        .iter()
        .map(|x| py::py_int(x).map(WriteValue::Int))
        .collect::<Result<Vec<_>, _>>()?;

        // pyaaf2 gives only the classes it knows its own Python class, and
        // upstream's `isinstance` checks are against those, so a subclass it
        // does not know is neither.
        match self.f.class_name(descriptor).as_str() {
            "CDCIDescriptor" => {
                let width = py::py_int(&get_or(&dict, "ComponentWidth", Any::Int(8)))?;
                self.f.set(descriptor, "ComponentWidth", width)?;
                let subsampling = py::py_int(&get_or(&dict, "HorizontalSubsampling", Any::Int(2)))?;
                self.f
                    .set(descriptor, "HorizontalSubsampling", subsampling)?;
            }
            "RGBADescriptor" => {
                // Upstream's workaround for pyaaf2 refusing the empty pixel
                // layout OTIO can hold.
                let default_layout = || {
                    WriteValue::array(["CompRed", "CompGreen", "CompBlue"].map(|code| {
                        WriteValue::record([
                            ("Code", WriteValue::from(code)),
                            ("Size", WriteValue::Int(8)),
                        ])
                    }))
                };
                let layout = match dict.get("PixelLayout") {
                    None => default_layout(),
                    Some(value) => {
                        let empty = match value {
                            Any::Vector(items) => items.is_empty(),
                            Any::Dictionary(d) => d.is_empty(),
                            Any::String(s) => s.is_empty(),
                            other => {
                                return Err(unwritable(format!(
                                    "object of type '{}' has no len()",
                                    other.type_name()
                                )));
                            }
                        };
                        if empty {
                            default_layout()
                        } else {
                            py::to_write_value(value)?
                                .ok_or_else(|| unwritable("a PixelLayout of None".to_owned()))?
                        }
                    }
                };
                self.f.set(descriptor, "PixelLayout", layout)?;
            }
            _ => {}
        }

        self.set_py(
            descriptor,
            "ImageAspectRatio",
            &get_or(&dict, "ImageAspectRatio", Any::String("16/9".to_owned())),
        )?;
        let width = py::py_int(&get_or(&dict, "StoredWidth", Any::Int(1920)))?;
        self.f.set(descriptor, "StoredWidth", width)?;
        let height = py::py_int(&get_or(&dict, "StoredHeight", Any::Int(1080)))?;
        self.f.set(descriptor, "StoredHeight", height)?;
        self.set_py(
            descriptor,
            "FrameLayout",
            &get_or(&dict, "FrameLayout", Any::String("FullFrame".to_owned())),
        )?;
        self.f
            .set(descriptor, "VideoLineMap", WriteValue::Array(linemap))?;

        // `str(AAFRational(x))`, which pyaaf2 parses back to the same
        // rational, so the rational is set as it is.
        let sample_rate = py::py_rational(&get_or(&dict, "SampleRate", Any::Int(24)))?;
        self.f.set(descriptor, "SampleRate", sample_rate)?;
        let length = py::py_int(&get_or(&dict, "Length", Any::Int(1)))?;
        self.f.set(descriptor, "Length", length)?;

        let media = media_reference(self.document, clip)?;
        if let Node::ExternalReference(external) = self.document.try_get(media)? {
            if !external.target_url.is_empty() {
                let locator = self.network_locator(&external.target_url)?;
                self.f.append(descriptor, "Locator", locator)?;
            }
            if let Some(available) = external.media.available_range {
                self.f
                    .set(descriptor, "SampleRate", available.duration().rate())?;
                let length = py::float_int(available.duration().value())?;
                self.f.set(descriptor, "Length", length)?;
            }
        }

        self.transcribe_otio_aaf_descriptor(descriptor, &dict)?;
        Ok(descriptor)
    }

    /// `AudioTrackTranscriber.default_descriptor`: a PCM descriptor, 16-bit
    /// mono at 48 kHz unless the metadata says otherwise.
    ///
    /// Upstream writes its defaults into the metadata dictionary and then
    /// copies the dictionary onto the descriptor, so where each lands among
    /// the other keys, and so the order the properties are set in, depends on
    /// whether the dictionary came from the metadata, whose keys stay sorted,
    /// or is the empty one it falls back to, whose keys stay in the order
    /// they were added. [`PyDict`] keeps that difference.
    pub(crate) fn audio_descriptor(&mut self, clip: NodeId) -> Result<ObjRef, Fail> {
        let descriptor = self.f.create("PCMDescriptor")?;
        let mut dict = essence_description(self, clip)?;

        let sample_rate = py::py_rational_float(&get_or(&dict, "SampleRate", Any::Int(48000)))?;
        let average_bps = py::py_int(&get_or(&dict, "AverageBPS", Any::Int(96000)))?;
        dict.set("AverageBPS", Any::Int(average_bps));
        let block_align = py::py_int(&get_or(&dict, "BlockAlign", Any::Int(2)))?;
        dict.set("BlockAlign", Any::Int(block_align));
        let bits = py::py_int(&get_or(&dict, "QuantizationBits", Any::Int(16)))?;
        dict.set("QuantizationBits", Any::Int(bits));

        let media = media_reference(self.document, clip)?;
        if let Node::ExternalReference(external) = self.document.try_get(media)? {
            // Upstream adds a locator here even for an empty URL.
            let locator = self.network_locator(&external.target_url)?;
            self.f.append(descriptor, "Locator", locator)?;
        }

        let sampling_rate =
            py::py_rational_float(&get_or(&dict, "AudioSamplingRate", Any::Int(48000)))?;
        dict.set("AudioSamplingRate", Any::Double(sampling_rate));
        let channels = py::py_int(&get_or(&dict, "Channels", Any::Int(1)))?;
        dict.set("Channels", Any::Int(channels));
        dict.set("SampleRate", Any::Double(sample_rate));

        // The default is worked out before the lookup, as Python evaluates
        // an argument, so media with no available range fails here even
        // when the metadata has a length.
        let available = self
            .document
            .try_get(media)?
            .media()
            .and_then(|m| m.available_range)
            .ok_or_else(|| {
                unwritable("'NoneType' object has no attribute 'duration'".to_owned())
            })?;
        let rescaled: RationalTime = available.duration().rescaled_to(sample_rate);
        let default_length = py::float_int(rescaled.value())?;
        let length = py::py_int(&get_or(&dict, "Length", Any::Int(default_length)))?;
        dict.set("Length", Any::Int(length));

        self.transcribe_otio_aaf_descriptor(descriptor, &dict)?;
        Ok(descriptor)
    }

    /// `aaf_network_locator`: a locator for a URL.
    fn network_locator(&mut self, url: &str) -> Result<ObjRef, Fail> {
        let locator = self.f.create("NetworkLocator")?;
        self.f.set(locator, "URLString", url)?;
        Ok(locator)
    }

    /// `transcribe_otio_aaf_descriptor`: every property the metadata names
    /// that the descriptor's class has and the descriptor does not have yet,
    /// set as pyaaf2 would store the metadata's value.
    ///
    /// Upstream logs and skips a key when pyaaf2 raises `KeyError`, which it
    /// does for a property the class does not have and for a record missing
    /// one of its members; anything else pyaaf2 refuses stops the write, and
    /// stops it here too.
    fn transcribe_otio_aaf_descriptor(
        &mut self,
        descriptor: ObjRef,
        dict: &PyDict,
    ) -> Result<(), Fail> {
        for (key, value) in dict.iter() {
            if key == "ClassName" || self.f.has(descriptor, key) {
                continue;
            }
            let type_name = match self.f.property_type_name(descriptor, key) {
                Ok(name) => name,
                Err(aaf::Error::UndefinedProperty { .. }) => continue,
                Err(other) => return Err(other.into()),
            };
            let result = if type_name == "AUID" {
                // `aaf2.types.AUID(value)`, which takes a string.
                let Any::String(text) = value else {
                    return Err(unwritable(format!(
                        "'{}' object has no attribute 'replace'",
                        value.type_name()
                    )));
                };
                let auid: Auid = text
                    .parse()
                    .map_err(|_| unwritable("badly formed hexadecimal UUID string".to_owned()))?;
                self.f.set(descriptor, key, auid)
            } else {
                match py::to_write_value(value)? {
                    Some(value) => self.f.set(descriptor, key, value),
                    // Setting `None` removes a property, and this one is not
                    // there to remove.
                    None => Ok(()),
                }
            };
            match result {
                Ok(())
                | Err(aaf::Error::MissingMember { .. } | aaf::Error::UndefinedProperty { .. }) => {}
                Err(other) => return Err(other.into()),
            }
        }
        Ok(())
    }
}
