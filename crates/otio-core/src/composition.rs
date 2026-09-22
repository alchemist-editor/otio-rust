//! Where things sit in time, and how compositions are edited.
//!
//! This is the machinery every algorithm, adapter and binding sits on: given
//! an object, what span of time does it occupy, and in whose coordinates? A
//! clip's `source_range` is stated in its media's time, its position on a
//! track is stated in the track's, and the track's position in a stack is
//! stated in the stack's, so almost every question needs a walk up or down
//! that chain.
//!
//! Upstream spreads these across `Composable`, `Item`, `Composition`, `Track`
//! and `Stack` as virtual methods. Here they are inherent methods on
//! [`Document`], because the object is a handle and the arena is what can
//! resolve it. The behaviour, including which cases are errors, follows
//! upstream exactly — an adapter that agrees with upstream on the object model
//! but not on `range_of_child` is not a port.

use std::collections::{BTreeMap, HashMap};

use opentime::{DEFAULT_EPSILON_S, RationalTime, TimeRange, max, min};

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::schema::{Node, Timeline};
use crate::value::Box2d;

/// Whether a composition lays its children out end to end or on top of one
/// another.
///
/// This is the distinction upstream draws with the `Track` and `Stack`
/// subclasses, and it is what every range question turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Children follow one another, as on a track.
    Sequential,
    /// Children share the same start, as in a stack.
    Layered,
}

/// What to do about a transition at the very start or end of a track when
/// asking for its neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NeighborGapPolicy {
    /// Report no neighbour, which is the literal truth.
    #[default]
    Never,
    /// Report a gap the length of the transition's overhang.
    ///
    /// A transition at the head of a track reaches backwards into time that
    /// has no item in it. Some algorithms want that time represented rather
    /// than absent, and upstream materializes a [`Gap`](crate::schema::Gap)
    /// for it.
    AroundTransitions,
}

impl Document {
    /// Returns how long an object occupies its parent's timeline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoDuration`] for an object that does not sit in time,
    /// such as a marker or an effect, and propagates whatever
    /// [`Document::trimmed_range`] reports for an item.
    pub fn duration(&self, id: NodeId) -> Result<RationalTime> {
        match self.try_get(id)? {
            Node::Transition(transition) => Ok(transition.in_offset + transition.out_offset),
            node if node.item().is_some() => Ok(self.trimmed_range(id)?.duration()),
            node => Err(Error::NoDuration {
                schema: node.schema_name().to_string(),
                object: id,
            }),
        }
    }

    /// Returns the full span of media or content behind an item, ignoring any
    /// trim.
    ///
    /// For a clip this is the active media reference's `available_range`. For
    /// a track it is the sum of its children, and for a stack the longest of
    /// them.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoAvailableRange`] for a clip whose media reference
    /// does not say how long the media is, and [`Error::NotImplemented`] for
    /// an object that cannot answer at all, such as a bare item or a gap.
    pub fn available_range(&self, id: NodeId) -> Result<TimeRange> {
        match self.try_get(id)? {
            Node::Clip(clip) => {
                // Upstream's `Clip::available_range` words the two ways this
                // can fail differently, and names the clip in both.
                let key = &clip.active_media_reference_key;
                let reference = clip
                    .media_references
                    .get(key)
                    .ok_or(Error::NoAvailableRange {
                        schema: "Clip".to_string(),
                        reason: "No media reference set on clip",
                        object: Some(id),
                    })?;
                self.try_get(*reference)?
                    .media()
                    .and_then(|media| media.available_range)
                    .ok_or(Error::NoAvailableRange {
                        schema: "Clip".to_string(),
                        reason: "No available_range set on media reference on clip",
                        object: Some(id),
                    })
            }
            Node::Track(track) => self.track_available_range(&track.children),
            Node::Stack(stack) => self.stack_available_range(&stack.children),
            // Only a clip, a track or a stack knows what sits behind it.
            // Upstream's `Item::available_range` reports NOT_IMPLEMENTED for
            // everything else, its own base `Item` included, rather than
            // pretending the answer is simply unknown.
            node => Err(Error::NotImplemented {
                operation: "available_range",
                schema: node.schema_name().to_string(),
            }),
        }
    }

    /// A track is as long as its items laid end to end, plus whatever a
    /// transition at either end hangs over by.
    fn track_available_range(&self, children: &[NodeId]) -> Result<TimeRange> {
        let mut duration = RationalTime::default();
        for child in children {
            if self.try_get(*child)?.item().is_some() {
                duration += self.duration(*child)?;
            }
        }

        // Resolved into an Option<&Node> first rather than written as a
        // let-chain: let-chains are stable from 1.88 and this crate builds on
        // 1.85.
        let first = children.first().map(|id| self.try_get(*id)).transpose()?;
        if let Some(Node::Transition(transition)) = first {
            duration += transition.in_offset;
        }
        let last = children.last().map(|id| self.try_get(*id)).transpose()?;
        if let Some(Node::Transition(transition)) = last {
            duration += transition.out_offset;
        }

        Ok(TimeRange::new(
            RationalTime::new(0.0, duration.rate()),
            duration,
        ))
    }

    /// A stack is as long as its longest layer.
    fn stack_available_range(&self, children: &[NodeId]) -> Result<TimeRange> {
        let Some((first, rest)) = children.split_first() else {
            return Ok(TimeRange::default());
        };

        let mut duration = self.duration(*first)?;
        for child in rest {
            duration = max(duration, self.duration(*child)?);
        }

        Ok(TimeRange::new(
            RationalTime::new(0.0, duration.rate()),
            duration,
        ))
    }

    /// Returns the span of an item actually in use: its `source_range` if it
    /// has one, and its full available range if not.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoAvailableRange`] for an untrimmed item whose extent
    /// nothing states.
    pub fn trimmed_range(&self, id: NodeId) -> Result<TimeRange> {
        match self.try_get(id)?.item().and_then(|item| item.source_range) {
            Some(range) => Ok(range),
            None => self.available_range(id),
        }
    }

    /// Returns an item's trimmed range widened by any transitions touching it.
    ///
    /// A clip under a dissolve is on screen for longer than its own range
    /// says, because the transition reaches into it from either side. This is
    /// that wider span.
    ///
    /// # Errors
    ///
    /// Propagates whatever [`Document::trimmed_range`] reports.
    pub fn visible_range(&self, id: NodeId) -> Result<TimeRange> {
        let mut result = self.trimmed_range(id)?;
        let Some(parent) = self.try_get(id)?.parent() else {
            return Ok(result);
        };

        let (head, tail) = self.handles_of_child(parent, id)?;
        if let Some(head) = head {
            result = TimeRange::new(result.start_time() - head, result.duration() + head);
        }
        if let Some(tail) = tail {
            result = TimeRange::new(result.start_time(), result.duration() + tail);
        }
        Ok(result)
    }

    /// Returns where an object sits in its parent's timeline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChild`] if the object has no parent.
    pub fn range_in_parent(&self, id: NodeId) -> Result<TimeRange> {
        let parent = self.parent_of_for(
            id,
            "cannot compute range in parent because item has no parent",
        )?;
        self.range_of_child(parent, id)
    }

    /// Returns where an object sits in its parent's timeline, clipped to the
    /// part of the parent that is itself in use.
    ///
    /// Returns `Ok(None)` when the accumulated range falls outside the
    /// composition's own trim, which only a nested child can do.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChild`] if the object has no parent, and
    /// [`Error::InvalidTimeRange`] if a direct child is trimmed out of its
    /// composition entirely. That last case is upstream's behaviour rather
    /// than an oversight: a caller handed a zero-length range instead would
    /// place the item at the head of the track.
    pub fn trimmed_range_in_parent(&self, id: NodeId) -> Result<Option<TimeRange>> {
        let parent = self.parent_of_for(
            id,
            "cannot compute trimmed range in parent because item has no parent",
        )?;
        self.trimmed_range_of_child(parent, id)
    }

    /// Returns the composition holding an object, as [`Document::parent_of`]
    /// does, but with upstream's words for a transition that has none.
    ///
    /// Upstream's `Transition` overrides the range-in-parent methods and
    /// adds `details` saying what could not be computed; an item's methods
    /// add nothing.
    fn parent_of_for(&self, id: NodeId, details: &'static str) -> Result<NodeId> {
        match self.parent_of(id) {
            Err(Error::NotAChild { schema, object, .. }) if schema == "Transition" => {
                Err(Error::NotAChild {
                    schema,
                    object,
                    details: Some(details),
                })
            }
            result => result,
        }
    }

    /// Returns the composition holding an object.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChild`] if it has none.
    pub fn parent_of(&self, id: NodeId) -> Result<NodeId> {
        let node = self.try_get(id)?;
        node.parent().ok_or_else(|| Error::NotAChild {
            schema: node.schema_name().to_string(),
            object: id,
            details: None,
        })
    }

    /// Returns the position of a child within a composition.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChildOf`] if the object is not among the
    /// composition's children.
    pub fn index_of_child(&self, parent: NodeId, child: NodeId) -> Result<usize> {
        let node = self.try_get(parent)?;
        node.children()
            .and_then(|children| children.iter().position(|each| *each == child))
            .ok_or_else(|| Error::NotAChildOf {
                parent: node.schema_name().to_string(),
                object: Some(parent),
            })
    }

    /// Returns whether an object is directly among a composition's children.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if the composition's handle is not live.
    pub fn has_child(&self, parent: NodeId, child: NodeId) -> Result<bool> {
        Ok(self
            .try_get(parent)?
            .children()
            .is_some_and(|children| children.contains(&child)))
    }

    /// Returns whether an object is a descendant of a composition, at any
    /// depth.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if a handle along the chain is not live.
    pub fn is_parent_of(&self, parent: NodeId, other: NodeId) -> Result<bool> {
        let mut current = self.try_get(other)?.parent();
        // A malformed parent chain must not spin forever; the arena makes a
        // cycle unreachable through the public API, but a handle can be
        // rewritten by hand.
        let mut steps = 0;
        while let Some(id) = current {
            if id == parent {
                return Ok(true);
            }
            if steps > self.len() {
                return Ok(false);
            }
            steps += 1;
            current = self.try_get(id)?.parent();
        }
        Ok(false)
    }

    /// Returns where the child at `index` sits in its composition's timeline.
    ///
    /// A negative index counts from the end, as upstream's does.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IllegalIndex`] if the index falls outside the
    /// composition, and [`Error::NotAComposition`] if the object holds no
    /// children.
    pub fn range_of_child_at_index(&self, parent: NodeId, index: i64) -> Result<TimeRange> {
        let (layout, children) = self.composition(parent)?;
        let index = adjusted_index(index, children.len())?;
        let child = children[index];
        let child_duration = self.duration(child)?;

        match layout {
            Layout::Layered => Ok(TimeRange::new(
                RationalTime::new(0.0, child_duration.rate()),
                child_duration,
            )),
            Layout::Sequential => {
                let mut start_time = RationalTime::new(0.0, child_duration.rate());
                for earlier in &children[..index] {
                    // A transition sits over its neighbours rather than
                    // beside them, so it does not advance the playhead.
                    if !matches!(self.try_get(*earlier)?, Node::Transition(_)) {
                        start_time += self.duration(*earlier)?;
                    }
                }
                if let Node::Transition(transition) = self.try_get(child)? {
                    start_time -= transition.in_offset;
                }
                Ok(TimeRange::new(start_time, child_duration))
            }
        }
    }

    /// Returns where the child at `index` sits, clipped to the part of the
    /// composition that is itself in use.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidTimeRange`] if the composition's own trim
    /// excludes the child entirely, matching upstream, which treats that as an
    /// error at this level while
    /// [`trimmed_range_of_child`](Document::trimmed_range_of_child) reports it
    /// as an absent range.
    pub fn trimmed_range_of_child_at_index(&self, parent: NodeId, index: i64) -> Result<TimeRange> {
        let range = self.range_of_child_at_index(parent, index)?;
        let (layout, _) = self.composition(parent)?;
        let Some(source_range) = self
            .try_get(parent)?
            .item()
            .and_then(|item| item.source_range)
        else {
            return Ok(range);
        };

        match layout {
            // A stack's layers all start together, so trimming moves them
            // to the trim's start and shortens them to fit.
            Layout::Layered => Ok(TimeRange::new(
                source_range.start_time(),
                min(range.duration(), source_range.duration()),
            )),
            Layout::Sequential => self
                .trim_child_range(parent, range)?
                .ok_or(Error::InvalidTimeRange),
        }
    }

    /// Clips a child's range to the composition's own `source_range`.
    ///
    /// Returns `Ok(None)` when the child falls entirely outside it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if the composition's handle is not live.
    pub fn trim_child_range(
        &self,
        parent: NodeId,
        mut child_range: TimeRange,
    ) -> Result<Option<TimeRange>> {
        let Some(source_range) = self
            .try_get(parent)?
            .item()
            .and_then(|item| item.source_range)
        else {
            return Ok(Some(child_range));
        };

        let past_end = source_range.start_time() >= child_range.end_time_exclusive();
        let before_start = source_range.end_time_exclusive() <= child_range.start_time();
        if past_end || before_start {
            return Ok(None);
        }

        if child_range.start_time() < source_range.start_time() {
            child_range = TimeRange::range_from_start_end_time(
                source_range.start_time(),
                child_range.end_time_exclusive(),
            );
        }
        if child_range.end_time_exclusive() > source_range.end_time_exclusive() {
            child_range = TimeRange::range_from_start_end_time(
                child_range.start_time(),
                source_range.end_time_exclusive(),
            );
        }
        Ok(Some(child_range))
    }

    /// Returns where a descendant sits in this composition's timeline.
    ///
    /// The descendant need not be a direct child: the ranges are accumulated
    /// down the chain from `parent`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotDescendedFrom`] if the object is not below
    /// `parent`.
    pub fn range_of_child(&self, parent: NodeId, child: NodeId) -> Result<TimeRange> {
        let mut result: Option<TimeRange> = None;
        let mut current = child;
        for ancestor in self.path_from_child(parent, child)? {
            let index = self.index_of_child(ancestor, current)?;
            let range = self.range_of_child_at_index(ancestor, index as i64)?;
            result = Some(match result {
                None => range,
                Some(inner) => {
                    TimeRange::new(inner.start_time() + range.start_time(), inner.duration())
                }
            });
            current = ancestor;
        }
        result.ok_or(Error::NotAChild {
            schema: self.try_get(child)?.schema_name().to_string(),
            object: child,
            details: None,
        })
    }

    /// Returns where a descendant sits in this composition's timeline, clipped
    /// to the part of the composition that is in use.
    ///
    /// Returns `Ok(None)` when the composition's trim excludes it entirely.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotDescendedFrom`] if the object is not below
    /// `parent`.
    pub fn trimmed_range_of_child(
        &self,
        parent: NodeId,
        child: NodeId,
    ) -> Result<Option<TimeRange>> {
        let mut result: Option<TimeRange> = None;
        let mut current = child;
        for ancestor in self.path_from_child(parent, child)? {
            let index = self.index_of_child(ancestor, current)?;
            let range = self.trimmed_range_of_child_at_index(ancestor, index as i64)?;
            result = Some(match result {
                None => range,
                Some(inner) => {
                    TimeRange::new(inner.start_time() + range.start_time(), inner.duration())
                }
            });
            current = ancestor;
        }

        let Some(result) = result else {
            return Err(Error::NotAChild {
                schema: self.try_get(child)?.schema_name().to_string(),
                object: child,
                details: None,
            });
        };

        let Some(source_range) = self
            .try_get(parent)?
            .item()
            .and_then(|item| item.source_range)
        else {
            return Ok(Some(result));
        };

        let new_start_time = max(source_range.start_time(), result.start_time());
        if new_start_time > result.end_time_exclusive() {
            return Ok(None);
        }

        let new_duration = min(
            result.end_time_exclusive(),
            source_range.end_time_exclusive(),
        ) - new_start_time;
        if new_duration.value() < 0.0 {
            return Ok(None);
        }
        Ok(Some(TimeRange::new(new_start_time, new_duration)))
    }

    /// Returns every child's range in this composition's timeline, keyed by
    /// handle.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] if the object holds no children.
    pub fn range_of_all_children(&self, parent: NodeId) -> Result<BTreeMap<NodeId, TimeRange>> {
        let (layout, children) = self.composition(parent)?;
        let mut result = BTreeMap::new();

        match layout {
            Layout::Layered => {
                for (index, child) in children.iter().enumerate() {
                    result.insert(*child, self.range_of_child_at_index(parent, index as i64)?);
                }
            }
            Layout::Sequential => {
                let Some(first) = children.first() else {
                    return Ok(result);
                };

                // Everything on the track is measured against the first
                // child's rate, so that laying items end to end does not
                // silently rescale as it goes.
                let rate = match self.try_get(*first)? {
                    Node::Transition(transition) => transition.in_offset.rate(),
                    node if node.item().is_some() => self.trimmed_range(*first)?.duration().rate(),
                    _ => 1.0,
                };

                let mut last_end_time = RationalTime::new(0.0, rate);
                for child in &children {
                    match self.try_get(*child)? {
                        Node::Transition(transition) => {
                            result.insert(
                                *child,
                                TimeRange::new(
                                    last_end_time - transition.in_offset,
                                    transition.out_offset + transition.in_offset,
                                ),
                            );
                        }
                        node if node.item().is_some() => {
                            let range = TimeRange::new(
                                last_end_time,
                                self.trimmed_range(*child)?.duration(),
                            );
                            result.insert(*child, range);
                            last_end_time = range.end_time_exclusive();
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(result)
    }

    /// Returns how far transitions reach into a child from either side.
    ///
    /// Only a sequential composition has transitions; a stack always reports
    /// no handles, as upstream does.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChildOf`] if the object is not among the
    /// composition's children.
    pub fn handles_of_child(
        &self,
        parent: NodeId,
        child: NodeId,
    ) -> Result<(Option<RationalTime>, Option<RationalTime>)> {
        let (layout, _) = self.composition(parent)?;
        if layout == Layout::Layered {
            return Ok((None, None));
        }

        let (before, after) = self.neighbors_of(parent, child)?;
        let head = match before {
            Some(id) => match self.try_get(id)? {
                Node::Transition(transition) => Some(transition.in_offset),
                _ => None,
            },
            None => None,
        };
        let tail = match after {
            Some(id) => match self.try_get(id)? {
                Node::Transition(transition) => Some(transition.out_offset),
                _ => None,
            },
            None => None,
        };
        Ok((head, tail))
    }

    /// Returns the children immediately before and after a child.
    ///
    /// With [`NeighborGapPolicy::AroundTransitions`], a transition at either
    /// end of the composition gets a [`Gap`](crate::schema::Gap) standing in
    /// for the time it hangs over into. That gap is inserted into the
    /// document, so this takes `&mut self`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChildOf`] if the object is not among the
    /// composition's children.
    pub fn neighbors_of_mut(
        &mut self,
        parent: NodeId,
        child: NodeId,
        policy: NeighborGapPolicy,
    ) -> Result<(Option<NodeId>, Option<NodeId>)> {
        let (_, children) = self.composition(parent)?;
        let index = self.index_of_child(parent, child)?;

        let offsets = match self.try_get(child)? {
            Node::Transition(transition) => Some((transition.in_offset, transition.out_offset)),
            _ => None,
        };

        let before = if index == 0 {
            match (policy, offsets) {
                (NeighborGapPolicy::AroundTransitions, Some((in_offset, _))) => {
                    Some(self.insert_gap(in_offset))
                }
                _ => None,
            }
        } else {
            Some(children[index - 1])
        };

        let after = if index + 1 == children.len() {
            match (policy, offsets) {
                (NeighborGapPolicy::AroundTransitions, Some((_, out_offset))) => {
                    Some(self.insert_gap(out_offset))
                }
                _ => None,
            }
        } else {
            Some(children[index + 1])
        };

        Ok((before, after))
    }

    /// Returns the children immediately before and after a child, reporting no
    /// neighbour where the composition ends.
    ///
    /// This is [`NeighborGapPolicy::Never`]. The gap-materializing policy
    /// needs to add an object to the document, so it lives on
    /// [`Document::neighbors_of_mut`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChildOf`] if the object is not among the
    /// composition's children.
    pub fn neighbors_of(
        &self,
        parent: NodeId,
        child: NodeId,
    ) -> Result<(Option<NodeId>, Option<NodeId>)> {
        let (_, children) = self.composition(parent)?;
        let index = self.index_of_child(parent, child)?;
        let before = (index > 0).then(|| children[index - 1]);
        let after = (index + 1 < children.len()).then(|| children[index + 1]);
        Ok((before, after))
    }

    /// Adds a gap of `duration` to the document and returns its handle.
    fn insert_gap(&mut self, duration: RationalTime) -> NodeId {
        let gap = crate::schema::Gap {
            item: crate::schema::ItemData {
                source_range: Some(TimeRange::new(
                    RationalTime::new(0.0, duration.rate()),
                    duration,
                )),
                ..crate::schema::ItemData::new()
            },
        };
        self.insert(Node::Gap(gap))
    }

    /// Returns the child covering `time`, descending into nested compositions
    /// unless `shallow` is set.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] if the object holds no children.
    pub fn child_at_time(
        &self,
        parent: NodeId,
        time: RationalTime,
        shallow: bool,
    ) -> Result<Option<NodeId>> {
        let ranges = self.range_of_all_children(parent)?;
        let (_, children) = self.composition(parent)?;

        let mut found = None;
        for child in &children {
            if ranges
                .get(child)
                .is_some_and(|range| range.overlaps_time(time))
            {
                found = Some(*child);
                break;
            }
        }

        let Some(found) = found else {
            return Ok(None);
        };
        if shallow || self.try_get(found)?.children().is_none() {
            return Ok(Some(found));
        }

        // The child keeps its own clock, so the search time has to be
        // restated in it before recursing.
        let inner_time = self.transformed_time(time, parent, found)?;
        self.child_at_time(found, inner_time, shallow)
    }

    /// Returns the children whose ranges meet `search_range`.
    ///
    /// This follows upstream in both of its forms. A stack keeps every item
    /// whose trimmed range in the stack intersects the search range. Any
    /// other composition bisects its children, which it assumes are laid end
    /// to end: it keeps the run from the first child whose inclusive end is
    /// not before the search's start to the last whose start is not after the
    /// search's inclusive end. The two disagree for a range of zero duration,
    /// which intersects nothing but still picks out the child under a point
    /// in a track — upstream's `top_clip_at_time` and its tests rely on that.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] if the object holds no children.
    pub fn children_in_range(
        &self,
        parent: NodeId,
        search_range: TimeRange,
    ) -> Result<Vec<NodeId>> {
        if let Node::Stack(stack) = self.try_get(parent)? {
            let mut found = Vec::new();
            for child in stack.children.clone() {
                if self.try_get(child)?.item().is_none() {
                    continue;
                }
                if self
                    .trimmed_range_in_parent(child)?
                    .is_some_and(|range| range.intersects(search_range, DEFAULT_EPSILON_S))
                {
                    found.push(child);
                }
            }
            return Ok(found);
        }

        let ranges = self.range_of_all_children(parent)?;
        let (_, children) = self.composition(parent)?;
        // Upstream reads the map with `operator[]`, which answers an empty
        // range for anything missing from it.
        let range_of = |child: &NodeId| ranges.get(child).copied().unwrap_or_default();

        let first = partition_from(&children, 0, |child| {
            range_of(child).end_time_inclusive() < search_range.start_time()
        });
        let last = partition_from(&children, first, |child| {
            range_of(child).start_time() <= search_range.end_time_inclusive()
        });
        Ok(children[first..last].to_vec())
    }

    /// Restates a time from one item's coordinates in another's.
    ///
    /// Both items must sit under a common ancestor. This is how a mark on a
    /// clip becomes a mark on the timeline that holds it.
    ///
    /// # Errors
    ///
    /// Propagates whatever the ranges along the path report.
    pub fn transformed_time(
        &self,
        time: RationalTime,
        from: NodeId,
        to: NodeId,
    ) -> Result<RationalTime> {
        let root = self.highest_ancestor(from)?;
        let mut result = time;

        // Walk up from `from`, converting out of each item's own clock and
        // into its parent's.
        let mut item = from;
        while item != root && item != to {
            let parent = self.parent_of(item)?;
            result -= self.trimmed_range(item)?.start_time();
            result += self.range_of_child(parent, item)?.start_time();
            item = parent;
        }

        // Then walk up from `to` the same way, undoing each step, which
        // leaves the time stated in `to`'s clock.
        let ancestor = item;
        let mut item = to;
        while item != root && item != ancestor {
            let parent = self.parent_of(item)?;
            result += self.trimmed_range(item)?.start_time();
            result -= self.range_of_child(parent, item)?.start_time();
            item = parent;
        }

        Ok(result)
    }

    /// Restates a range from one item's coordinates in another's.
    ///
    /// # Errors
    ///
    /// Propagates whatever [`Document::transformed_time`] reports.
    pub fn transformed_time_range(
        &self,
        range: TimeRange,
        from: NodeId,
        to: NodeId,
    ) -> Result<TimeRange> {
        Ok(TimeRange::new(
            self.transformed_time(range.start_time(), from, to)?,
            range.duration(),
        ))
    }

    /// Returns the topmost object above `id`, which is `id` itself if it has
    /// no parent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if a handle along the chain is not live.
    pub fn highest_ancestor(&self, id: NodeId) -> Result<NodeId> {
        let mut current = id;
        let mut steps = 0;
        while let Some(parent) = self.try_get(current)?.parent() {
            current = parent;
            steps += 1;
            if steps > self.len() {
                break;
            }
        }
        Ok(current)
    }

    /// Returns every descendant an object holds that `matches`, in document
    /// order.
    ///
    /// With a `search_range`, only children meeting that range are considered,
    /// and the range is restated in each nested composition's own clock as the
    /// search descends. With `shallow` set, the search stops at the direct
    /// children.
    ///
    /// A serializable collection is searched the way upstream searches one:
    /// every child is considered, and the range is handed unchanged to each
    /// collection, composition or timeline below it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] if the object holds no children.
    ///
    /// # Note on a difference from upstream
    ///
    /// Upstream's `find_children` reassigns the search range in place as it
    /// recurses, so a range transformed for one nested composition leaks into
    /// the siblings examined after it. That is an aliasing slip rather than
    /// intended behaviour — the transformed range is meaningless outside the
    /// composition it was computed for. Here each child gets the range
    /// restated in its own clock, so results differ from upstream only for a
    /// ranged, non-shallow search across more than one nested composition.
    pub fn find_children<F>(
        &self,
        parent: NodeId,
        search_range: Option<TimeRange>,
        shallow: bool,
        matches: &F,
    ) -> Result<Vec<NodeId>>
    where
        F: Fn(&Node) -> bool,
    {
        if let Node::SerializableCollection(collection) = self.try_get(parent)? {
            let children = collection.children.clone();
            return self.find_children_in_collection(&children, search_range, shallow, matches);
        }

        let children = match search_range {
            Some(range) => self.children_in_range(parent, range)?,
            None => self.children_of(parent)?,
        };

        let mut found = Vec::new();
        for child in children {
            if matches(self.try_get(child)?) {
                found.push(child);
            }
            if shallow || self.try_get(child)?.children().is_none() {
                continue;
            }

            let inner_range = match search_range {
                Some(range) => Some(self.transformed_time_range(range, parent, child)?),
                None => None,
            };
            found.extend(self.find_children(child, inner_range, shallow, matches)?);
        }
        Ok(found)
    }

    /// [`Document::find_children`] for the children of a serializable
    /// collection.
    ///
    /// A collection has no timing, so this follows upstream's
    /// `SerializableCollection::find_children` rather than the composition
    /// search: every child is considered whatever the range, and the range is
    /// handed unchanged to each nested collection, composition or timeline,
    /// which reads it in its own clock. A timeline is searched through its
    /// stack of tracks, as upstream's `Timeline::find_children` does, so the
    /// stack itself is never a match.
    fn find_children_in_collection<F>(
        &self,
        children: &[NodeId],
        search_range: Option<TimeRange>,
        shallow: bool,
        matches: &F,
    ) -> Result<Vec<NodeId>>
    where
        F: Fn(&Node) -> bool,
    {
        let mut found = Vec::new();
        for child in children.iter().copied() {
            let node = self.try_get(child)?;
            if matches(node) {
                found.push(child);
            }
            if shallow {
                continue;
            }
            let below = match node {
                Node::SerializableCollection(_)
                | Node::Track(_)
                | Node::Stack(_)
                | Node::Composition(_) => child,
                Node::Timeline(Timeline {
                    tracks: Some(tracks),
                    ..
                }) => *tracks,
                _ => continue,
            };
            found.extend(self.find_children(below, search_range, false, matches)?);
        }
        Ok(found)
    }

    /// Returns the image bounds of the media behind an object.
    ///
    /// A clip reports its active media reference's bounds. A track unions the
    /// bounds of the clips directly on it, and a stack unions those of every
    /// clip below it — that difference is upstream's, not a simplification
    /// here. Anything else does not have media at all.
    ///
    /// `None` means a composition holds no clips to ask.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoImageBounds`] for a clip whose media does not say
    /// what its bounds are, and for a track or stack holding such a clip;
    /// and [`Error::NotImplemented`] for an object that is not a clip or a
    /// composition, which is what upstream's base class reports.
    pub fn available_image_bounds(&self, id: NodeId) -> Result<Option<Box2d>> {
        let node = self.try_get(id)?;
        let clips = match node {
            // Upstream's `Clip` reports bounds it cannot find as an error
            // rather than as nothing, and a track or stack holding such a
            // clip passes the error on.
            Node::Clip(clip) => {
                let key = &clip.active_media_reference_key;
                let Some(reference) = clip.media_references.get(key) else {
                    return Err(Error::NoImageBounds {
                        reason: "No image bounds set on clip",
                        object: id,
                    });
                };
                return self
                    .try_get(*reference)?
                    .media()
                    .and_then(|media| media.available_image_bounds)
                    .map(Some)
                    .ok_or(Error::NoImageBounds {
                        reason: "No image bounds set on media reference on clip",
                        object: id,
                    });
            }
            // A track asks only the clips sitting directly on it; a nested
            // track's clips are not its own.
            Node::Track(track) => track.children.clone(),
            Node::Stack(_) | Node::Composition(_) => self.find_clips(id)?,
            node => {
                return Err(Error::NotImplemented {
                    operation: "available_image_bounds",
                    schema: node.schema_name().to_string(),
                });
            }
        };

        let mut bounds: Option<Box2d> = None;
        for child in clips {
            if !matches!(self.try_get(child)?, Node::Clip(_)) {
                continue;
            }
            let Some(child_bounds) = self.available_image_bounds(child)? else {
                continue;
            };
            bounds = Some(match bounds {
                None => child_bounds,
                Some(bounds) => bounds.extended_by(child_bounds),
            });
        }
        Ok(bounds)
    }

    /// Returns every clip below an object, in document order.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if a handle along the way is not live.
    pub fn find_clips(&self, id: NodeId) -> Result<Vec<NodeId>> {
        let mut found = Vec::new();
        self.collect_clips(id, &mut found)?;
        Ok(found)
    }

    fn collect_clips(&self, id: NodeId, found: &mut Vec<NodeId>) -> Result<()> {
        let node = self.try_get(id)?;
        if matches!(node, Node::Clip(_)) {
            found.push(id);
            return Ok(());
        }
        if let Node::Timeline(timeline) = node {
            // A timeline owns its stack without it being a child, so the walk
            // has to step through it explicitly. One with no stack holds
            // nothing.
            return match timeline.tracks {
                Some(tracks) => self.collect_clips(tracks, found),
                None => Ok(()),
            };
        }
        let Some(children) = node.children() else {
            return Ok(());
        };
        for child in children.iter().copied() {
            self.collect_clips(child, found)?;
        }
        Ok(())
    }

    /// Returns an object's children.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] for an object that holds none.
    pub fn children_of(&self, id: NodeId) -> Result<Vec<NodeId>> {
        let node = self.try_get(id)?;
        node.children()
            .map(<[NodeId]>::to_vec)
            .ok_or_else(|| Error::NotAComposition {
                schema: node.schema_name().to_string(),
            })
    }

    /// Returns the chain of compositions from `child`'s parent up to and
    /// including `parent`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotDescendedFrom`] if the walk reaches the top without
    /// meeting `parent`.
    fn path_from_child(&self, parent: NodeId, child: NodeId) -> Result<Vec<NodeId>> {
        let mut current = self.parent_of(child)?;
        let mut path = vec![current];
        while current != parent {
            current = self
                .try_get(current)?
                .parent()
                .ok_or_else(|| Error::NotDescendedFrom {
                    parent: self
                        .get(parent)
                        .map_or("composition", Node::schema_name)
                        .to_string(),
                    object: Some(parent),
                })?;
            path.push(current);
        }
        Ok(path)
    }

    /// Returns the composition's layout and its children.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] for an object that holds no
    /// children, and [`Error::NoLayout`] for a bare `Composition`, which
    /// holds children but does not say where they sit. Upstream reports the
    /// second as `NOT_IMPLEMENTED` from the base class's
    /// `range_of_child_at_index`.
    fn composition(&self, id: NodeId) -> Result<(Layout, Vec<NodeId>)> {
        match self.try_get(id)? {
            Node::Track(track) => Ok((Layout::Sequential, track.children.clone())),
            Node::Stack(stack) => Ok((Layout::Layered, stack.children.clone())),
            Node::Composition(_) => Err(Error::NoLayout),
            node => Err(Error::NotAComposition {
                schema: node.schema_name().to_string(),
            }),
        }
    }

    /// Returns a composition's children, whether or not it has a layout.
    ///
    /// Editing a composition's children does not need to know where they
    /// sit, so a bare `Composition` can be edited even though it cannot be
    /// asked for a range.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] for an object that holds no
    /// children.
    fn composition_children(&self, id: NodeId) -> Result<Vec<NodeId>> {
        self.children_of(id)
    }
}

/// Editing a composition's children, and copying a subtree.
impl Document {
    /// Puts `child` into `parent` at `index`, moving later children along.
    ///
    /// A negative index counts from the end, and an index at or past the end
    /// appends.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ChildAlreadyParented`] if the object already sits in a
    /// composition, and [`Error::NotAComposition`] if `parent` holds no
    /// children. Upstream refuses the same case rather than silently
    /// re-parenting, because the object would then appear in two places.
    pub fn insert_child(&mut self, parent: NodeId, index: i64, child: NodeId) -> Result<()> {
        if self.try_get(child)?.parent().is_some() {
            return Err(Error::ChildAlreadyParented);
        }

        let len = self.composition_children(parent)?.len();
        let len_i64 = i64::try_from(len).unwrap_or(i64::MAX);
        let resolved = if index < 0 { index + len_i64 } else { index };
        let at = usize::try_from(resolved.clamp(0, len_i64)).unwrap_or(len);

        self.try_get_mut(child)?.set_parent(Some(parent));
        match self.try_get_mut(parent)? {
            Node::Track(track) => track.children.insert(at, child),
            Node::Stack(stack) => stack.children.insert(at, child),
            Node::Composition(composition) => composition.children.insert(at, child),
            Node::SerializableCollection(collection) => collection.children.insert(at, child),
            _ => unreachable!("composition() has already rejected everything else"),
        }
        Ok(())
    }

    /// Adds `child` to the end of `parent`'s children.
    ///
    /// # Errors
    ///
    /// As [`Document::insert_child`].
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<()> {
        let len = i64::try_from(self.composition_children(parent)?.len()).unwrap_or(i64::MAX);
        self.insert_child(parent, len, child)
    }

    /// Takes the child at `index` out of `parent` and returns it.
    ///
    /// The object stays in the document, parentless, so it can be put
    /// somewhere else. Dropping it from the document is a separate
    /// [`Document::remove`] call.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IllegalIndex`] if the index falls outside the
    /// composition.
    pub fn remove_child(&mut self, parent: NodeId, index: i64) -> Result<NodeId> {
        let len = self.composition_children(parent)?.len();
        let at = adjusted_index(index, len)?;

        let child = match self.try_get_mut(parent)? {
            Node::Track(track) => track.children.remove(at),
            Node::Stack(stack) => stack.children.remove(at),
            Node::Composition(composition) => composition.children.remove(at),
            Node::SerializableCollection(collection) => collection.children.remove(at),
            _ => unreachable!("composition() has already rejected everything else"),
        };
        self.try_get_mut(child)?.set_parent(None);
        Ok(child)
    }

    /// Takes `child` out of `parent`, wherever it sits.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAChildOf`] if the object is not among the
    /// composition's children.
    pub fn detach_child(&mut self, parent: NodeId, child: NodeId) -> Result<()> {
        let index = self.index_of_child(parent, child)?;
        self.remove_child(parent, index as i64)?;
        Ok(())
    }

    /// Removes every child from `parent`, leaving them parentless in the
    /// document.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotAComposition`] if `parent` holds no children.
    pub fn clear_children(&mut self, parent: NodeId) -> Result<Vec<NodeId>> {
        let children = self.composition_children(parent)?;
        for child in &children {
            self.try_get_mut(*child)?.set_parent(None);
        }
        match self.try_get_mut(parent)? {
            Node::Track(track) => track.children.clear(),
            Node::Stack(stack) => stack.children.clear(),
            Node::Composition(composition) => composition.children.clear(),
            Node::SerializableCollection(collection) => collection.children.clear(),
            _ => unreachable!("composition() has already rejected everything else"),
        }
        Ok(children)
    }

    /// Drops an object and everything below it from the document.
    ///
    /// Every handle into the removed subtree goes stale, which is what makes
    /// this safe to call on scratch objects an algorithm built along the way.
    /// The exception is an object named to [`Document::spare`]: it is left
    /// in the document, with no parent and everything below it intact.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if a handle in the subtree is not live.
    pub fn remove_recursive(&mut self, id: NodeId) -> Result<()> {
        if self.spared(id) {
            return Ok(());
        }
        let Some(node) = self.get(id) else {
            return Ok(());
        };

        let mut owned: Vec<NodeId> = Vec::new();
        if let Some(item) = node.item() {
            owned.extend(&item.effects);
            owned.extend(&item.markers);
        }
        match node {
            Node::Clip(clip) => owned.extend(clip.media_references.values()),
            Node::Timeline(timeline) => owned.extend(timeline.tracks),
            _ => {}
        }
        if let Some(children) = node.children() {
            owned.extend(children);
        }

        for child in owned {
            self.remove_recursive(child)?;
        }
        self.remove(id);
        Ok(())
    }

    /// Copies an object and everything below it, returning the copy's handle.
    ///
    /// This is upstream's `clone()`. Every owned object gets a fresh handle:
    /// children, effects, markers and media references are copied too, so the
    /// copy shares nothing with the original and can be edited freely. The
    /// copy has no parent, whatever the original had.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if a handle in the subtree is not live.
    pub fn deep_clone(&mut self, id: NodeId) -> Result<NodeId> {
        self.deep_clone_tracked(id, &mut HashMap::new())
    }

    /// [`Document::deep_clone`], remembering what it has already copied.
    ///
    /// `copies` maps each original to its copy, and an original is entered in
    /// it as soon as its shell is in the arena, before any of its links are
    /// followed. Metadata can hold a whole object, and nothing stops that
    /// object being one the copy is inside — `clip.metadata["self"] = clip` is
    /// enough — so without the map the walk would follow that link for ever
    /// and take the process down with it. Sharing the map across the whole
    /// walk also means an object held twice is copied once, so the copy has
    /// the same sharing the original did.
    fn deep_clone_tracked(
        &mut self,
        id: NodeId,
        copies: &mut HashMap<NodeId, NodeId>,
    ) -> Result<NodeId> {
        if let Some(copy) = copies.get(&id) {
            return Ok(*copy);
        }

        let mut node = self.try_get(id)?.clone();
        node.set_parent(None);
        let new_id = self.insert(node);
        copies.insert(id, new_id);

        // Each link is taken out of the copy, followed, and put back. Taking
        // it out first keeps the borrow of the document short enough to
        // recurse under.
        let (effects, markers) = match self.try_get_mut(new_id)?.item_mut() {
            Some(item) => (
                std::mem::take(&mut item.effects),
                std::mem::take(&mut item.markers),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let mut new_effects = Vec::with_capacity(effects.len());
        for effect in effects {
            new_effects.push(self.deep_clone_tracked(effect, copies)?);
        }
        let mut new_markers = Vec::with_capacity(markers.len());
        for marker in markers {
            new_markers.push(self.deep_clone_tracked(marker, copies)?);
        }
        if let Some(item) = self.try_get_mut(new_id)?.item_mut() {
            item.effects = new_effects;
            item.markers = new_markers;
        }

        // Metadata, and a generator's parameters, may hold whole objects. They
        // are owned, so a deep copy owes the caller its own; otherwise writing
        // through the copy would reach into the original.
        let mut held = Vec::new();
        self.try_get_mut(new_id)?
            .visit_held_objects_mut(&mut |id| held.push(*id));
        let mut copied_held = Vec::with_capacity(held.len());
        for id in held {
            copied_held.push(self.deep_clone_tracked(id, copies)?);
        }
        let mut copied_held = copied_held.into_iter();
        self.try_get_mut(new_id)?.visit_held_objects_mut(&mut |id| {
            if let Some(copy) = copied_held.next() {
                *id = copy;
            }
        });

        let references = match self.try_get_mut(new_id)? {
            Node::Clip(clip) => std::mem::take(&mut clip.media_references),
            _ => BTreeMap::new(),
        };
        let mut copied_references = BTreeMap::new();
        for (key, reference) in references {
            copied_references.insert(key, self.deep_clone_tracked(reference, copies)?);
        }
        if let Node::Clip(clip) = self.try_get_mut(new_id)? {
            clip.media_references = copied_references;
        }

        let tracks = match self.try_get_mut(new_id)? {
            Node::Timeline(timeline) => timeline.tracks.take(),
            _ => None,
        };
        if let Some(tracks) = tracks {
            let copied = self.deep_clone_tracked(tracks, copies)?;
            if let Node::Timeline(timeline) = self.try_get_mut(new_id)? {
                timeline.tracks = Some(copied);
            }
        }

        let children = match self.try_get_mut(new_id)? {
            Node::Track(track) => std::mem::take(&mut track.children),
            Node::Stack(stack) => std::mem::take(&mut stack.children),
            Node::Composition(composition) => std::mem::take(&mut composition.children),
            Node::SerializableCollection(collection) => std::mem::take(&mut collection.children),
            _ => Vec::new(),
        };
        let mut copied_children = Vec::with_capacity(children.len());
        for child in children {
            copied_children.push(self.deep_clone_tracked(child, copies)?);
        }
        for child in &copied_children {
            self.try_get_mut(*child)?.set_parent(Some(new_id));
        }
        match self.try_get_mut(new_id)? {
            Node::Track(track) => track.children = copied_children,
            Node::Stack(stack) => stack.children = copied_children,
            Node::Composition(composition) => composition.children = copied_children,
            Node::SerializableCollection(collection) => collection.children = copied_children,
            _ => {}
        }

        // A timeline's stack is owned but is not a child in the composition
        // sense, so its parent link stays empty, as upstream leaves it.
        Ok(new_id)
    }
}

/// Upstream's `_bisect_left` and `_bisect_right`, as one function.
///
/// Returns the first index at or after `from` where `before` stops holding,
/// by binary search, so it assumes `before` holds for a prefix of the slice
/// and not after. Upstream's children are not guaranteed to be ordered that
/// way and neither are these; the search answers what upstream's would.
fn partition_from<T>(items: &[T], from: usize, before: impl Fn(&T) -> bool) -> usize {
    let (mut low, mut high) = (from, items.len());
    while low < high {
        let middle = low + (high - low) / 2;
        if before(&items[middle]) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

/// Resolves a possibly negative index against a length, as upstream's
/// `adjusted_vector_index` does: `-1` is the last child.
fn adjusted_index(index: i64, len: usize) -> Result<usize> {
    let len_i64 = i64::try_from(len).unwrap_or(i64::MAX);
    let resolved = if index < 0 { index + len_i64 } else { index };
    if resolved < 0 || resolved >= len_i64 {
        return Err(Error::IllegalIndex { index, len });
    }
    usize::try_from(resolved).map_err(|_| Error::IllegalIndex { index, len })
}

#[cfg(test)]
mod tests {
    use super::adjusted_index;

    #[test]
    fn a_negative_index_counts_from_the_end() {
        assert_eq!(adjusted_index(-1, 3), Ok(2));
        assert_eq!(adjusted_index(-3, 3), Ok(0));
        assert_eq!(adjusted_index(0, 3), Ok(0));
    }

    #[test]
    fn an_index_outside_the_composition_is_an_error() {
        assert!(adjusted_index(3, 3).is_err());
        assert!(adjusted_index(-4, 3).is_err());
        assert!(adjusted_index(0, 0).is_err());
    }
}
