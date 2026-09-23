//! Embedding essence: pyaaf2's `import_dnxhd_essence` and
//! `import_audio_essence`, on master mobs and source mobs.
//!
//! A master mob imports essence through a new source mob, named after it
//! with `.PHYS` added, which holds the essence descriptor and slot 1 of the
//! essence; the essence itself goes in an `EssenceData` filed under the
//! source mob's `MobID` in the content storage, as a stream. The master mob
//! gets a new timeline slot with a clip of the source mob's slot.
//!
//! Both imports write the essence the way pyaaf2 writes it, a piece at a
//! time — a frame at a time for DNxHD, a second of samples at a time for a
//! WAV file — because where each piece lands in the compound file follows
//! from the pieces it was written in.

use std::path::Path;

use super::dnx::Frames;
use super::value::{Rational, WriteValue};
use super::wave::Wave;
use super::{AafWriter, ObjRef};
use crate::builtin::auid;
use crate::error::{Error, Result};
use crate::{Auid, MobId};

/// The DNxHD codec definition pyaaf2 gives a DNxHD descriptor.
const DNXHD_CODEC: Auid = auid("8ef593f6-9521-4344-9ede-b84e8cfdc7da");

/// How essence to import is described: as pyaaf2's two `import_*_essence`
/// methods take it, beyond the file.
#[derive(Debug, Clone, Copy, Default)]
pub struct EssenceImport {
    /// The edit rate of the slots made for the essence. A DNxHD import needs
    /// one; an audio import takes the file's sample rate if not given one.
    pub edit_rate: Option<Rational>,
    /// A clip of the tape the essence came from, which becomes the source
    /// mob's slot 1 segment in place of the clip to nothing it starts with.
    pub tape: Option<ObjRef>,
    /// The length to give that segment, if not the essence's own. pyaaf2
    /// treats a length of zero as none.
    pub length: Option<i64>,
    /// Describe the essence without embedding it: no `EssenceData` is made.
    pub offline: bool,
}

/// Reads a whole media file, as pyaaf2 opens it.
fn read_media(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| Error::Media {
        path: path.display().to_string(),
        source,
    })
}

/// `float(value)` of a rational, as pyaaf2's `rescale` takes it.
#[allow(clippy::cast_precision_loss)]
fn as_f64(rate: Rational) -> f64 {
    rate.numerator as f64 / rate.denominator as f64
}

impl AafWriter {
    /// Imports a raw DNxHD or DNxHR stream: pyaaf2's
    /// `mob.import_dnxhd_essence(path, edit_rate, tape, length, offline)`.
    ///
    /// On a master mob this makes the source mob and returns the master
    /// mob's new slot. On a source mob it describes and embeds the essence
    /// in the mob itself, and returns its slot 1.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob is neither a master mob nor a source mob,
    /// if `import.edit_rate` is missing, if the file cannot be read, or if it
    /// is not a DNx stream pyaaf2 can import, with pyaaf2's message.
    pub fn import_dnxhd_essence(
        &mut self,
        mob: ObjRef,
        path: impl AsRef<Path>,
        import: EssenceImport,
    ) -> Result<ObjRef> {
        let path = path.as_ref();
        let edit_rate = import.edit_rate.ok_or(Error::Unsupported {
            what: "importing DNxHD essence without an edit rate",
        })?;
        if self.is_a(mob, "SourceMob") {
            return self.source_import_dnxhd(mob, path, edit_rate, import);
        }
        let source_mob = self.physical_source_mob(mob)?;
        let source_slot = self.source_import_dnxhd(source_mob, path, edit_rate, import)?;
        self.clip_of_physical_slot(mob, source_mob, source_slot, edit_rate, "picture")
    }

    /// Imports a WAV file's samples: pyaaf2's
    /// `mob.import_audio_essence(path, edit_rate, tape, length, offline)`.
    ///
    /// As for [`import_dnxhd_essence`](Self::import_dnxhd_essence). Without
    /// an edit rate the slots run at the file's sample rate.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob is neither a master mob nor a source mob,
    /// if the file cannot be read, or if it is not a PCM WAV file pyaaf2 can
    /// import, with the message Python's `wave` module gives.
    pub fn import_audio_essence(
        &mut self,
        mob: ObjRef,
        path: impl AsRef<Path>,
        import: EssenceImport,
    ) -> Result<ObjRef> {
        let path = path.as_ref();
        if self.is_a(mob, "SourceMob") {
            return self.source_import_audio(mob, path, import);
        }
        let source_mob = self.physical_source_mob(mob)?;
        let source_slot = self.source_import_audio(source_mob, path, import)?;
        let edit_rate = match import.edit_rate {
            Some(rate) => rate,
            None => self.edit_rate(source_slot)?,
        };
        self.clip_of_physical_slot(mob, source_mob, source_slot, edit_rate, "sound")
    }

    /// The start of a master mob's import: a new source mob named after the
    /// master mob, `"%s.PHYS" % self.name`, put in the file.
    fn physical_source_mob(&mut self, master_mob: ObjRef) -> Result<ObjRef> {
        if !self.is_a(master_mob, "MasterMob") {
            return Err(Error::WrongClass {
                expected: "MasterMob or SourceMob".to_owned(),
                found: self.class_name_of(master_mob),
            });
        }
        // Python formats a missing name as `None`.
        let name = self
            .get_string(master_mob, "Name")?
            .unwrap_or_else(|| "None".to_owned());
        let source_mob = self.create_mob("SourceMob", Some(&format!("{name}.PHYS")))?;
        self.add_mob(source_mob)?;
        Ok(source_mob)
    }

    /// The end of a master mob's import: a new timeline slot on the master
    /// mob holding a clip of the source mob's slot, as long as that slot's
    /// segment.
    fn clip_of_physical_slot(
        &mut self,
        master_mob: ObjRef,
        source_mob: ObjRef,
        source_slot: ObjRef,
        edit_rate: Rational,
        media_kind: &str,
    ) -> Result<ObjRef> {
        let slot = self.create_timeline_slot(master_mob, edit_rate, None)?;
        let slot_id = self.slot_id(source_slot)?;
        let clip =
            self.create_mob_source_clip(source_mob, slot_id, None, None, Some(media_kind))?;
        self.set(slot, "Segment", clip)?;
        let source_segment = self.segment_of(source_slot)?;
        let length = self.get_int(source_segment, "Length")?.unwrap_or(0);
        self.set(clip, "Length", length)?;
        Ok(slot)
    }

    fn slot_id(&self, slot: ObjRef) -> Result<u32> {
        self.get_int(slot, "SlotID")?
            .and_then(|id| u32::try_from(id).ok())
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(slot),
                property: "SlotID".to_owned(),
            })
    }

    fn segment_of(&self, slot: ObjRef) -> Result<ObjRef> {
        self.get_object(slot, "Segment")?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(slot),
                property: "Segment".to_owned(),
            })
    }

    /// A slot's edit rate: pyaaf2's `slot.edit_rate`.
    fn edit_rate(&self, slot: ObjRef) -> Result<Rational> {
        let data = self
            .get_bytes(slot, "EditRate")?
            .and_then(|d| <[u8; 8]>::try_from(d).ok())
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(slot),
                property: "EditRate".to_owned(),
            })?;
        let numerator = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let denominator = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        Ok(Rational::new(numerator.into(), denominator.into()))
    }

    /// pyaaf2's `SourceMob.create_essence`: slot 1, holding a clip to
    /// nothing, and unless the essence is offline an `EssenceData` filed
    /// under the mob's `MobID`.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob already has a slot 1 or has no `MobID`.
    pub fn create_essence(
        &mut self,
        source_mob: ObjRef,
        edit_rate: impl Into<Rational>,
        media_kind: &str,
        offline: bool,
    ) -> Result<(Option<ObjRef>, ObjRef)> {
        // pyaaf2 notes that a source mob appears to link to one essence only,
        // and in slot 1.
        let slot = self.create_empty_slot(source_mob, edit_rate, Some(media_kind), Some(1))?;
        if offline {
            return Ok((None, slot));
        }
        let essence = self.create("EssenceData")?;
        let mob_id: MobId =
            self.get_mob_id(source_mob, "MobID")?
                .ok_or_else(|| Error::MissingProperty {
                    class: self.class_name_of(source_mob),
                    property: "MobID".to_owned(),
                })?;
        self.set(essence, "MobID", mob_id)?;
        let content = self.content()?;
        self.append(content, "EssenceData", essence)?;
        Ok((Some(essence), slot))
    }

    /// `SourceMob.import_dnxhd_essence`.
    fn source_import_dnxhd(
        &mut self,
        mob: ObjRef,
        path: &Path,
        edit_rate: Rational,
        import: EssenceImport,
    ) -> Result<ObjRef> {
        let (essence, slot) = self.create_essence(mob, edit_rate, "picture", import.offline)?;
        if let Some(tape) = import.tape {
            self.set(slot, "Segment", tape)?;
        }

        let descriptor = self.create("CDCIDescriptor")?;
        self.set(mob, "EssenceDescription", descriptor)?;
        self.set(descriptor, "SampleRate", edit_rate)?;
        // pyaaf2 is not sure what the line map should be either.
        self.set(
            descriptor,
            "VideoLineMap",
            WriteValue::Array(vec![WriteValue::Int(42), WriteValue::Int(0)]),
        )?;
        let container = self.lookup_containerdef("AAF")?;
        self.set(descriptor, "ContainerFormat", container)?;
        let codec = self.lookup_codecdef(DNXHD_CODEC)?;
        self.set(descriptor, "CodecDefinition", codec)?;

        let stream = match essence {
            Some(essence) => {
                let spec = self.find(essence, "Data")?;
                Some(self.open_stream_prop(essence, &spec)?)
            }
            None => None,
        };

        let file = read_media(path)?;
        let mut count = 0i64;
        let mut described = false;
        for frame in Frames::new(&file) {
            let (header, packet) = frame?;
            count += 1;
            if !described {
                described = true;
                self.set(descriptor, "StoredWidth", header.width)?;
                self.set(descriptor, "StoredHeight", header.height)?;
                self.set(descriptor, "ComponentWidth", header.bitdepth)?;
                let layout = if header.interlaced {
                    "SeparateFields"
                } else {
                    "FullFrame"
                };
                self.set(descriptor, "FrameLayout", layout)?;
                self.set(
                    descriptor,
                    "ImageAspectRatio",
                    format!("{}/{}", header.width, header.height),
                )?;
                self.set(descriptor, "FrameSampleSize", packet.len() as i64)?;
                self.set(descriptor, "Compression", header.compression()?)?;
                self.set(descriptor, "HorizontalSubsampling", 2)?;
            }
            if let Some(stream) = stream {
                self.cfb.append_stream(stream, packet)?;
            }
        }
        if count == 0 {
            // pyaaf2 fails here too, reading the frame count it never set.
            return Err(Error::InvalidMedia {
                reason: format!("{} holds no DNxHD frames", path.display()),
            });
        }

        let segment = self.segment_of(slot)?;
        let length = import.length.filter(|l| *l != 0).unwrap_or(count);
        self.set(segment, "Length", length)?;
        self.set(descriptor, "Length", count)?;
        Ok(slot)
    }

    /// `SourceMob.import_audio_essence`.
    fn source_import_audio(
        &mut self,
        mob: ObjRef,
        path: &Path,
        import: EssenceImport,
    ) -> Result<ObjRef> {
        let file = read_media(path)?;
        let mut wave = Wave::parse(&file)?;
        let sample_rate = i64::from(wave.sample_rate);
        let edit_rate = import
            .edit_rate
            .filter(|rate| rate.numerator != 0)
            .unwrap_or_else(|| Rational::new(sample_rate, 1));

        let (essence, slot) = self.create_essence(mob, edit_rate, "sound", import.offline)?;
        if let Some(tape) = import.tape {
            self.set(slot, "Segment", tape)?;
        }

        let descriptor = self.create("PCMDescriptor")?;
        self.set(mob, "EssenceDescription", descriptor)?;
        self.set(descriptor, "Channels", wave.channels)?;
        self.set(descriptor, "BlockAlign", wave.block_align)?;
        self.set(descriptor, "SampleRate", sample_rate)?;
        let bytes_per_second =
            sample_rate * i64::from(wave.channels) * i64::from(wave.sample_width);
        self.set(descriptor, "AverageBPS", bytes_per_second)?;
        self.set(descriptor, "QuantizationBits", wave.sample_width * 8)?;
        self.set(descriptor, "AudioSamplingRate", sample_rate)?;

        #[allow(clippy::cast_possible_wrap)]
        let frames = wave.frames as i64;
        self.set(descriptor, "Length", frames)?;
        let length = match import.length.filter(|l| *l != 0) {
            Some(length) => length,
            // `int(rescale(frames, sample_rate, edit_rate))`, in floating
            // point as pyaaf2 computes it.
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            None => (frames as f64 * as_f64(edit_rate) / sample_rate as f64) as i64,
        };
        let segment = self.segment_of(slot)?;
        self.set(segment, "Length", length)?;

        if let Some(essence) = essence {
            let spec = self.find(essence, "Data")?;
            let stream = self.open_stream_prop(essence, &spec)?;
            // A second of samples at a time.
            let second = wave.sample_rate as usize;
            loop {
                let data = wave.read_frames(second);
                if data.is_empty() {
                    break;
                }
                self.cfb.append_stream(stream, data)?;
            }
        }
        Ok(slot)
    }
}
