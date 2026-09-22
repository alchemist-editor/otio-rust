//! The edit operations an editorial tool performs on a track.
//!
//! This is upstream's `algo/editAlgorithm.cpp`: overwrite, insert, trim,
//! slice, slip, slide, ripple, roll, fill and remove. Each takes the document
//! by mutable reference and edits a composition in place.
//!
//! The diagrams in each operation's documentation are upstream's, kept because
//! they say in three lines what a paragraph does not.
//!
//! Every operation here works in rounded time. Upstream compares durations
//! against a fixed epsilon rather than zero, because a duration that arrives
//! as the difference of two large frame counts can miss zero by a hair, and a
//! zero-length item inserted into a track is a real defect. [`is_zero`] is
//! that comparison.
//!
//! # The copies
//!
//! [`slice()`], [`insert`], [`overwrite`] and [`fill`] copy an item — the
//! piece of a split item left over, or the clip dropped into a gap. Upstream
//! makes that copy with `clone()`, and the copy here is made the same way
//! ([`Document::clone_object`]): an object the item holds in two places,
//! such as one object under two metadata keys, one effect listed twice or one
//! media reference under two keys, becomes two objects in the copy, and
//! nothing in the copy is shared with the original. The item left in place
//! keeps what it held.
//!
//! # Where these differ from upstream
//!
//! The copy follows a cycle in the item's metadata, so an item that holds
//! itself (`clip.metadata["self"] = clip`) is edited like any other and its
//! copy holds itself in turn. Upstream refuses such an item. It makes the
//! copy with `clone()`, which writes the object out and reads it back and
//! so cannot carry a cycle: the edit fails with `OBJECT_CYCLE`, raised in
//! Python as `ValueError`. That is a limit of how upstream copies, not of the
//! edit, and three of the four fail only after changing the track, leaving
//! the item cut short and the rest of it gone; `fill` copies first and fails
//! cleanly. Refusing here would turn a sound edit into an error for no other
//! reason, so the edit is allowed. The document still cannot be written as
//! JSON while the cycle is there, here or upstream.
//!
//! (Upstream's C++ tests of this, the four "fails gracefully" regression
//! tests, check for `TYPE_MISMATCH`. That comes from their storing a
//! `Retainer<Clip>` in metadata, a type upstream's writer has no entry for,
//! and not from the cycle; nothing here can hold such a value.)

use opentime::{RationalTime, TimeRange};

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::schema::{EffectData, Gap, ItemData, Node};

/// The largest difference that still counts as zero.
///
/// Upstream's value and reasoning: at one million seconds, in double
/// precision, the smallest number that can be added to one million and give
/// back something other than one million is about 5.82e-11. Nothing here is
/// tested beyond a million seconds.
pub const DOUBLE_EPSILON: f64 = 5.820_77e-11;

/// Returns whether a value is zero to within [`DOUBLE_EPSILON`].
#[must_use]
pub fn is_zero(value: f64) -> bool {
    value.abs() <= DOUBLE_EPSILON
}

/// Which clock a three- or four-point edit lines its media up against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReferencePoint {
    /// Use the media's own timing, and take as much of it as fits.
    #[default]
    Source,
    /// Line the media up against the track, trimming it to the gap.
    Sequence,
    /// Stretch or squeeze the media to fill the gap exactly.
    Fit,
}

/// Returns an item's available range, or an empty one where nothing states it.
///
/// Upstream's `available_range` reports failure through an error status and
/// returns a default-constructed range, and every caller in this module then
/// tests the duration against zero to decide whether clamping applies. This
/// crate returns an error instead, so the same decision is spelled out here.
fn available_or_empty(document: &Document, id: NodeId) -> TimeRange {
    document.available_range(id).unwrap_or_default()
}

/// Adds a gap covering `range`, or returns the caller's template.
fn fill_item(document: &mut Document, template: Option<NodeId>, range: TimeRange) -> NodeId {
    template.unwrap_or_else(|| {
        document.insert(Node::Gap(Gap {
            item: ItemData {
                source_range: Some(range),
                ..ItemData::new()
            },
        }))
    })
}

/// Sets an item's source range.
fn set_source_range(document: &mut Document, id: NodeId, range: TimeRange) -> Result<()> {
    document
        .try_get_mut(id)?
        .item_mut()
        .ok_or(Error::NotAnItem)?
        .source_range = Some(range);
    Ok(())
}

/// Removes every transition meeting `range` from a composition.
fn remove_transitions_in(
    document: &mut Document,
    composition: NodeId,
    range: TimeRange,
) -> Result<()> {
    let transitions = document.find_children(composition, Some(range), true, &|node| {
        matches!(node, Node::Transition(_))
    })?;
    for transition in transitions {
        if let Ok(index) = document.index_of_child(composition, transition) {
            let removed = document.remove_child(composition, index as i64)?;
            document.remove_recursive(removed)?;
        }
    }
    Ok(())
}

/// Overwrites whatever is under `range` with `item`.
///
/// ```text
/// | A | B |  ->  |A| C |B|
///   ^   ^
///   | C |
/// ```
///
/// An overwrite starting past the end of the composition appends, filling the
/// space between with `fill_template` or a gap. One starting before the
/// beginning inserts at the head the same way.
///
/// # Errors
///
/// Returns [`Error::NotAnItem`] if the range covers nothing that can be
/// overwritten.
///
/// An item whose metadata holds itself is copied with the cycle intact,
/// where upstream refuses it with `OBJECT_CYCLE`: see [the module
/// documentation](self#where-these-differ-from-upstream).
pub fn overwrite(
    document: &mut Document,
    item: NodeId,
    composition: NodeId,
    range: TimeRange,
    remove_transitions: bool,
    fill_template: Option<NodeId>,
) -> Result<()> {
    let composition_range = document.trimmed_range(composition)?;
    let start_time = range.start_time();

    if start_time >= composition_range.end_time_exclusive() {
        let fill_duration = start_time - composition_range.end_time_exclusive();
        if !is_zero(fill_duration.value()) {
            let fill_range =
                TimeRange::new(RationalTime::new(0.0, fill_duration.rate()), fill_duration);
            let filler = fill_item(document, fill_template, fill_range);
            document.append_child(composition, filler)?;
        }
        return document.append_child(composition, item);
    }

    if start_time < composition_range.start_time()
        && range.end_time_exclusive() < composition_range.start_time()
    {
        let fill_duration = composition_range.start_time() - start_time - range.duration();
        if !is_zero(fill_duration.value()) {
            let fill_range =
                TimeRange::new(RationalTime::new(0.0, fill_duration.rate()), fill_duration);
            let filler = fill_item(document, fill_template, fill_range);
            document.insert_child(composition, 0, filler)?;
        }
        return document.insert_child(composition, 0, item);
    }

    if remove_transitions {
        remove_transitions_in(document, composition, range)?;
    }

    let mut items = document.find_children(composition, Some(range), true, &|node| {
        node.item().is_some()
    })?;
    let Some(&first) = items.first() else {
        return Err(Error::NotAnItem);
    };

    let mut item_range = document
        .trimmed_range_of_child(composition, first)?
        .ok_or(Error::InvalidTimeRange)?;

    if items.len() == 1 && item_range.contains_range(range, 0.0) {
        return overwrite_within_one_item(document, item, composition, range, first, item_range);
    }

    // The overwrite spans several items: the ones at either end may be only
    // partly covered and survive shortened, everything between them goes.
    let mut insert_index = document.index_of_child(composition, first)?;
    let mut first_partial = None;
    if item_range.start_time() < range.start_time() {
        let trimmed = document.trimmed_range(first)?;
        first_partial = Some(TimeRange::new(
            trimmed.start_time(),
            range.start_time() - item_range.start_time(),
        ));
        insert_index += 1;
    }

    let last = *items.last().expect("items is not empty");
    let mut last_partial = None;
    item_range = document
        .trimmed_range_of_child(composition, last)?
        .ok_or(Error::InvalidTimeRange)?;
    if item_range.end_time_inclusive() > range.end_time_inclusive() {
        let trimmed = document.trimmed_range(last)?;
        let mut duration = item_range.end_time_inclusive() - range.end_time_inclusive();
        last_partial = Some(if items.len() == 1 {
            duration += range.start_time();
            TimeRange::new(trimmed.start_time() + range.duration(), duration)
        } else {
            TimeRange::new(
                trimmed.start_time() + duration,
                trimmed.duration() - duration,
            )
        });
    }

    if let Some(source_range) = first_partial {
        set_source_range(document, first, source_range)?;
        items.remove(0);
    }
    if let Some(source_range) = last_partial {
        if let Some(&last) = items.last() {
            set_source_range(document, last, source_range)?;
            items.pop();
        }
    }

    // Whatever is left is covered end to end, so it goes.
    while let Some(covered) = items.pop() {
        let index = document.index_of_child(composition, covered)?;
        let removed = document.remove_child(composition, index as i64)?;
        document.remove_recursive(removed)?;
    }

    let trimmed = document.trimmed_range(item)?;
    set_source_range(
        document,
        item,
        TimeRange::new(trimmed.start_time(), range.duration()),
    )?;
    document.insert_child(composition, insert_index as i64, item)
}

/// The case where the overwrite falls wholly inside a single item, which is
/// therefore split in two around it.
fn overwrite_within_one_item(
    document: &mut Document,
    item: NodeId,
    composition: NodeId,
    range: TimeRange,
    first: NodeId,
    item_range: TimeRange,
) -> Result<()> {
    // Dropping a clip with a time warp onto a gap is `fill`'s Fit case
    // arriving here, and there the incoming item's length is the answer
    // rather than something to trim.
    let is_fill_fit = matches!(document.try_get(first)?, Node::Gap(_))
        && document.try_get(item)?.item().is_some_and(|data| {
            data.effects.iter().any(|effect| {
                matches!(
                    document.get(*effect),
                    Some(Node::LinearTimeWarp { .. } | Node::FreezeFrame { .. })
                )
            })
        });

    let first_duration = range.start_time() - item_range.start_time();
    let second_duration = item_range.duration() - range.duration() - first_duration;
    let first_index = document.index_of_child(composition, first)?;
    let mut insert_index = first_index;
    let mut trimmed = document.trimmed_range(first)?;

    if is_zero(first_duration.value()) {
        // The overwrite starts exactly where the item does, so nothing of it
        // is left in front.
        let removed = document.remove_child(composition, first_index as i64)?;
        // Kept alive: the tail half below is cloned from it.
        document.try_get_mut(removed)?.set_parent(None);
    } else {
        set_source_range(
            document,
            first,
            TimeRange::new(trimmed.start_time(), first_duration),
        )?;
        insert_index += 1;
    }

    let incoming = document.trimmed_range(item)?;
    if range.duration() < incoming.duration() && !is_fill_fit {
        set_source_range(
            document,
            item,
            TimeRange::new(incoming.start_time(), range.duration()),
        )?;
    }
    document.insert_child(composition, insert_index as i64, item)?;

    if !is_zero(second_duration.value()) {
        let second = document.clone_object_keeping_cycles(first)?;
        trimmed = document.trimmed_range(second)?;
        set_source_range(
            document,
            second,
            TimeRange::new(
                trimmed.start_time() + first_duration + range.duration(),
                second_duration,
            ),
        )?;
        insert_index += 1;
        document.insert_child(composition, insert_index as i64, second)?;
    }

    if is_zero(first_duration.value()) {
        document.remove_recursive(first)?;
    }
    Ok(())
}

/// Inserts `item` at `time`, splitting whatever is there and pushing the rest
/// along.
///
/// ```text
/// |     A     | B |  ->  | A | C | A | B |
///       ^
///     | C |
/// ```
///
/// # Errors
///
/// Returns [`Error::NotAComposition`] if the target holds no children.
///
/// An item whose metadata holds itself is copied with the cycle intact,
/// where upstream refuses it with `OBJECT_CYCLE`: see [the module
/// documentation](self#where-these-differ-from-upstream).
pub fn insert(
    document: &mut Document,
    item: NodeId,
    composition: NodeId,
    time: RationalTime,
    remove_transitions: bool,
    fill_template: Option<NodeId>,
) -> Result<()> {
    if remove_transitions {
        let at = TimeRange::new(time, RationalTime::new(1.0, time.rate()));
        remove_transitions_in(document, composition, at)?;
    }

    let composition_range = document.trimmed_range(composition)?;
    let existing = document
        .child_at_time(composition, time, false)?
        .filter(|id| document.get(*id).is_some_and(|node| node.item().is_some()));

    let Some(existing) = existing else {
        if time >= composition_range.end_time_exclusive() {
            let fill_duration = time - composition_range.end_time_exclusive();
            if !is_zero(fill_duration.value()) {
                let fill_range =
                    TimeRange::new(RationalTime::new(0.0, fill_duration.rate()), fill_duration);
                let filler = fill_item(document, fill_template, fill_range);
                document.append_child(composition, filler)?;
            }
            return document.append_child(composition, item);
        }
        if time < composition_range.start_time() {
            return document.insert_child(composition, 0, item);
        }
        return Err(Error::NotAnItem);
    };

    let index = document.index_of_child(composition, existing)?;
    let range = document.trimmed_range_of_child_at_index(composition, index as i64)?;
    let mut insert_index = index;

    let first_source_range = TimeRange::new(
        document.trimmed_range(existing)?.start_time(),
        time - range.start_time(),
    );
    let split = !is_zero(first_source_range.duration().value());
    if split {
        set_source_range(document, existing, first_source_range)?;
        insert_index += 1;
    }

    document.insert_child(composition, insert_index as i64, item)?;
    if !split {
        return Ok(());
    }

    let insert_range =
        document.trimmed_range_of_child_at_index(composition, insert_index as i64)?;
    let second_source_range = TimeRange::new(
        first_source_range.start_time() + insert_range.start_time() + insert_range.duration(),
        range.end_time_exclusive() - time,
    );
    if is_zero(second_source_range.duration().value()) {
        return Ok(());
    }

    let second = document.clone_object_keeping_cycles(existing)?;
    set_source_range(document, second, second_source_range)?;
    document.insert_child(composition, insert_index as i64 + 1, second)
}

/// Adjusts one item's start or end without moving anything else.
///
/// ```text
/// |    A    | B | C |  ->  |  A  |FILL| B | C |
///        <--*
/// ```
///
/// The time the item gives up becomes a gap, unless the item next to it is
/// already a gap, in which case that gap grows instead.
///
/// # Errors
///
/// Returns [`Error::NotAChild`] if the item is not in a composition.
pub fn trim(
    document: &mut Document,
    item: NodeId,
    delta_in: RationalTime,
    delta_out: RationalTime,
    fill_template: Option<NodeId>,
) -> Result<()> {
    let composition = document.parent_of(item)?;
    let children = document.children_of(composition)?;
    let index = document.index_of_child(composition, item)?;

    let range = document.trimmed_range(item)?;
    let mut start_time = range.start_time();
    let mut end_time_exclusive = range.end_time_exclusive();

    if delta_in.value() != 0.0 {
        start_time += delta_in;
        if index > 0 {
            let previous = children[index - 1];
            let previous_range = document.trimmed_range(previous)?;
            set_source_range(
                document,
                previous,
                TimeRange::new(
                    previous_range.start_time(),
                    previous_range.duration() + delta_in,
                ),
            )?;
        }
    }

    if delta_out.value() != 0.0 {
        let next_index = index + 1;
        if let Some(&next) = children.get(next_index) {
            let next_is_gap = matches!(document.try_get(next)?, Node::Gap(_));
            if next_is_gap && delta_out.value() > 0.0 {
                end_time_exclusive += delta_out;
            } else if delta_out.value() < 0.0 {
                end_time_exclusive += delta_out;
                if next_is_gap {
                    // The gap next door absorbs the time rather than a new
                    // one appearing beside it.
                    let gap_range = document.trimmed_range(next)?;
                    set_source_range(
                        document,
                        next,
                        TimeRange::new(
                            gap_range.start_time() - delta_out,
                            gap_range.duration() + delta_out,
                        ),
                    )?;
                } else {
                    let fill_duration = -delta_out;
                    if fill_duration.value() > 0.0 {
                        let fill_range = TimeRange::new(
                            RationalTime::new(0.0, fill_duration.rate()),
                            fill_duration,
                        );
                        let filler = fill_item(document, fill_template, fill_range);
                        document.insert_child(composition, next_index as i64, filler)?;
                    }
                }
            }
        }
    }

    set_source_range(
        document,
        item,
        TimeRange::range_from_start_end_time(start_time, end_time_exclusive),
    )
}

/// Cuts the item at `time` in two.
///
/// ```text
/// | A | B | -> |A|A| B |
///   ^
/// ```
///
/// A cut exactly on an item's start does nothing, since there would be no
/// first half.
///
/// # Errors
///
/// Returns [`Error::NotAnItem`] if nothing is under `time`, and
/// [`Error::CannotTrimTransition`] if a transition covers it and
/// `remove_transitions` is not set.
///
/// An item whose metadata holds itself is copied with the cycle intact,
/// where upstream refuses it with `OBJECT_CYCLE`: see [the module
/// documentation](self#where-these-differ-from-upstream).
pub fn slice(
    document: &mut Document,
    composition: NodeId,
    time: RationalTime,
    remove_transitions: bool,
) -> Result<()> {
    let item = document
        .child_at_time(composition, time, false)?
        .filter(|id| document.get(*id).is_some_and(|node| node.item().is_some()))
        .ok_or(Error::NotAnItem)?;

    let index = document.index_of_child(composition, item)?;
    let range = document.trimmed_range_of_child_at_index(composition, index as i64)?;

    let duration = time - range.start_time();
    if is_zero(duration.value()) {
        return Ok(());
    }

    // A cut inside a transition would leave it reaching into an item that is
    // no longer there.
    let mut covering = Vec::new();
    if let Ok((before, after)) = document.neighbors_of(composition, item) {
        for neighbour in [after, before].into_iter().flatten() {
            if !matches!(document.try_get(neighbour)?, Node::Transition(_)) {
                continue;
            }
            if document
                .trimmed_range_of_child(composition, neighbour)?
                .is_some_and(|range| range.contains_time(time))
            {
                covering.push(neighbour);
            }
        }
    }

    if !covering.is_empty() {
        if !remove_transitions {
            return Err(Error::CannotTrimTransition);
        }
        for transition in covering {
            let index = document.index_of_child(composition, transition)?;
            let removed = document.remove_child(composition, index as i64)?;
            document.remove_recursive(removed)?;
        }
    }

    let first_source_range = TimeRange::new(document.trimmed_range(item)?.start_time(), duration);
    set_source_range(document, item, first_source_range)?;

    let second_source_range = TimeRange::new(
        first_source_range.start_time() + first_source_range.duration(),
        range.duration() - first_source_range.duration(),
    );
    if is_zero(second_source_range.duration().value()) {
        return Ok(());
    }

    let second = document.clone_object_keeping_cycles(item)?;
    set_source_range(document, second, second_source_range)?;
    let index = document.index_of_child(composition, item)?;
    document.insert_child(composition, index as i64 + 1, second)
}

/// Moves which part of its media an item shows, without moving the item.
///
/// ```text
/// |   A   |
///  <----->
/// ```
///
/// Clamped to the media's available range where there is one, so slipping
/// cannot run off either end of the media.
///
/// # Errors
///
/// Returns [`Error::NotAnItem`] if the handle is not an item.
pub fn slip(document: &mut Document, item: NodeId, delta: RationalTime) -> Result<()> {
    let range = document.trimmed_range(item)?;
    let mut start_time = range.start_time() + delta;

    let available = available_or_empty(document, item);
    if !is_zero(available.duration().value()) {
        if start_time < available.start_time() {
            start_time = available.start_time();
        } else if start_time + range.duration() > available.end_time_exclusive() {
            // Pull back so the end lands on the end of the media.
            start_time =
                start_time - (start_time + range.duration() - available.end_time_exclusive());
        }
    }

    set_source_range(document, item, TimeRange::new(start_time, range.duration()))
}

/// Moves an item along the track by stretching the one before it.
///
/// ```text
/// | A | B | C |  ->  | A     | B | C |
///     *--->
/// ```
///
/// Does nothing to the first item on a track, which has nothing to push
/// against.
///
/// # Errors
///
/// Returns [`Error::StaleHandle`] if the handle is not live.
pub fn slide(document: &mut Document, item: NodeId, delta: RationalTime) -> Result<()> {
    let Ok(composition) = document.parent_of(item) else {
        return Ok(());
    };
    let index = document.index_of_child(composition, item)?;
    if index == 0 || delta.value() == 0.0 {
        return Ok(());
    }

    let children = document.children_of(composition)?;
    let previous = children[index - 1];
    let range = document.trimmed_range(previous)?;
    let available = available_or_empty(document, previous);
    let mut offset = delta;

    if delta.value() < 0.0 {
        // Moving left cannot swallow the previous item whole.
        if range.duration() <= -delta {
            return Ok(());
        }
    } else if !is_zero(available.duration().value())
        && range.duration() + delta > available.duration()
    {
        offset = available.duration() - range.duration();
    }

    set_source_range(
        document,
        previous,
        TimeRange::new(range.start_time(), range.duration() + offset),
    )
}

/// Adjusts an item's source range without touching anything around it.
///
/// ```text
/// |   A   |   B   |  ->  | A |  B  |FILL|
///      <--*
/// ```
///
/// # Errors
///
/// Returns [`Error::NotAnItem`] if the handle is not an item.
pub fn ripple(
    document: &mut Document,
    item: NodeId,
    delta_in: RationalTime,
    delta_out: RationalTime,
) -> Result<()> {
    let range = document.trimmed_range(item)?;
    let mut start_time = range.start_time();
    let mut end_time_exclusive = range.end_time_exclusive();

    if delta_in.value() != 0.0 {
        let mut in_offset = delta_in;
        if delta_in < start_time {
            in_offset = -start_time;
        } else if start_time + delta_in > end_time_exclusive {
            in_offset = delta_in - end_time_exclusive;
        }
        start_time += in_offset;
    }

    if delta_out.value() != 0.0 {
        let mut out_offset = delta_out;
        if delta_out.value() > 0.0 {
            let available = available_or_empty(document, item);
            if !is_zero(available.duration().value())
                && range.duration() + delta_out > available.duration()
            {
                out_offset = available.duration() - range.duration();
            }
        }
        end_time_exclusive += out_offset;
    }

    set_source_range(
        document,
        item,
        TimeRange::range_from_start_end_time(start_time, end_time_exclusive),
    )
}

/// Moves the cut between an item and its neighbour, keeping the track the same
/// length.
///
/// ```text
/// |   A   |   B   |  ->  | A |  B      |
///      <--*
/// ```
///
/// No new items appear and nothing beyond the two either side moves.
///
/// # Errors
///
/// Returns [`Error::NotAChild`] if the item is not in a composition.
pub fn roll(
    document: &mut Document,
    item: NodeId,
    delta_in: RationalTime,
    delta_out: RationalTime,
) -> Result<()> {
    let composition = document.parent_of(item)?;
    let children = document.children_of(composition)?;
    let index = document.index_of_child(composition, item)?;

    let range = document.trimmed_range(item)?;
    let available = available_or_empty(document, item);
    let mut start_time = range.start_time();
    let mut end_time_exclusive = range.end_time_exclusive();

    if delta_in.value() != 0.0 {
        let mut in_offset = delta_in;
        if -in_offset > start_time {
            in_offset = -start_time;
        }
        if index > 0 {
            let previous = children[index - 1];
            let previous_range = document.trimmed_range(previous)?;

            // The previous item cannot be rolled away entirely; it keeps at
            // least one frame.
            let mut duration = previous_range.duration();
            if duration < -in_offset {
                duration -= RationalTime::new(1.0, duration.rate());
                in_offset -= duration;
            }
            set_source_range(
                document,
                previous,
                TimeRange::new(
                    previous_range.start_time(),
                    previous_range.duration() + in_offset,
                ),
            )?;
        }
        start_time += in_offset;

        if !is_zero(available.duration().value()) && start_time < available.start_time() {
            start_time = available.start_time();
        }
    }

    if delta_out.value() != 0.0 {
        let next_index = index + 1;
        if let Some(&next) = children.get(next_index) {
            let next_range = document.trimmed_range(next)?;
            let next_available = available_or_empty(document, next);
            let mut next_start_time = next_range.start_time();
            let mut out_offset = delta_out;

            if is_zero(available.duration().value()) {
                if -out_offset > next_start_time {
                    out_offset = -next_start_time;
                }
            } else if -out_offset > next_available.start_time() {
                out_offset = -next_available.start_time();
            }

            end_time_exclusive += out_offset;
            next_start_time += out_offset;
            set_source_range(
                document,
                next,
                TimeRange::new(next_start_time, next_range.duration()),
            )?;
        }
    }

    set_source_range(
        document,
        item,
        TimeRange::range_from_start_end_time(start_time, end_time_exclusive),
    )
}

/// Drops an item into the gap at `track_time`: a three- or four-point edit.
///
/// ```text
/// | A |GAP| B |  ->  | A | C | B |
///     ^   ^
///  C--| C |--C
/// ```
///
/// [`ReferencePoint`] decides how the media is lined up: by its own timing, by
/// the track's, or stretched to fill the gap exactly.
///
/// # Errors
///
/// Returns [`Error::NotAGap`] if there is no gap at `track_time`.
///
/// An item whose metadata holds itself is copied with the cycle intact,
/// where upstream refuses it with `OBJECT_CYCLE`: see [the module
/// documentation](self#where-these-differ-from-upstream).
pub fn fill(
    document: &mut Document,
    item: NodeId,
    track: NodeId,
    track_time: RationalTime,
    reference_point: ReferencePoint,
) -> Result<()> {
    let gap = document
        .child_at_time(track, track_time, true)?
        .filter(|id| matches!(document.get(*id), Some(Node::Gap(_))))
        .ok_or(Error::NotAGap)?;

    let clip_range = document.trimmed_range(item)?;
    let gap_range = document.trimmed_range(gap)?;
    let gap_track_range = document
        .trimmed_range_of_child(track, gap)?
        .ok_or(Error::InvalidTimeRange)?;
    let mut duration = clip_range.duration();

    match reference_point {
        ReferencePoint::Source => {
            let range = TimeRange::new(track_time, duration);
            overwrite(document, item, track, range, true, None)
        }
        ReferencePoint::Sequence => {
            let mut start_time = clip_range.start_time();
            let copy = document.clone_object_keeping_cycles(item)?;

            if start_time < gap_range.start_time() {
                duration -= gap_range.start_time() - start_time;
                start_time = gap_range.start_time();
            }
            if clip_range.end_time_exclusive() > gap_range.end_time_exclusive() {
                duration = gap_range.end_time_exclusive() - start_time;
            }
            set_source_range(document, copy, TimeRange::new(start_time, duration))?;

            if duration > gap_track_range.end_time_exclusive() - track_time {
                duration = gap_track_range.end_time_exclusive() - track_time;
            }
            let range = TimeRange::new(track_time, duration);
            overwrite(document, copy, track, range, true, None)
        }
        ReferencePoint::Fit => {
            // Stretch the media to the gap by hanging a time warp off a bare
            // item that carries the clip's range and effects.
            let percent = gap_range.duration().to_seconds() / duration.to_seconds();
            let name = document.try_get(item)?.name().to_string();
            let warp = document.insert(Node::LinearTimeWarp {
                effect: EffectData {
                    base: crate::schema::Base {
                        name: name.clone(),
                        ..crate::schema::Base::default()
                    },
                    effect_name: format!("{name}_timeWarp"),
                    enabled: true,
                },
                time_scalar: percent,
            });

            let mut effects = document
                .try_get(item)?
                .item()
                .map(|data| data.effects.clone())
                .unwrap_or_default();
            effects.push(warp);

            let fitted = document.insert(Node::Item(ItemData {
                base: crate::schema::Base {
                    name,
                    ..crate::schema::Base::default()
                },
                source_range: Some(clip_range),
                effects,
                ..ItemData::new()
            }));

            let range = TimeRange::new(
                track_time,
                gap_track_range.end_time_exclusive() - track_time,
            );
            overwrite(document, fitted, track, range, true, None)
        }
    }
}

/// Takes out whatever is at `time`, optionally leaving a gap of the same
/// length.
///
/// ```text
/// | A | C | B |  ->  | A |GAP| B |
///       ^
/// ```
///
/// Without `fill`, the items either side become neighbours and the track gets
/// shorter.
///
/// # Errors
///
/// Returns [`Error::NotAnItem`] if nothing is under `time`.
pub fn remove(
    document: &mut Document,
    composition: NodeId,
    time: RationalTime,
    fill: bool,
    fill_template: Option<NodeId>,
) -> Result<()> {
    let item = document
        .child_at_time(composition, time, false)?
        .filter(|id| document.get(*id).is_some_and(|node| node.item().is_some()))
        .ok_or(Error::NotAnItem)?;

    let index = document.index_of_child(composition, item)?;
    let item_range = document.trimmed_range(item)?;
    let removed = document.remove_child(composition, index as i64)?;
    document.remove_recursive(removed)?;

    if fill {
        let filler = fill_item(document, fill_template, item_range);
        document.insert_child(composition, index as i64, filler)?;
    }
    Ok(())
}
