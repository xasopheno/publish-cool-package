use std::{collections::VecDeque, iter::FromIterator};

use crate::{
    changelog::{section, section::Segment, Section, Version},
    ChangeLog,
};

impl ChangeLog {
    /// Bring `generated` into `self` in such a way that `self` preserves everything while enriching itself from `generated`.
    /// Thus we clearly assume that `self` is parsed and `generated` is generated.
    pub fn merge_generated(mut self, rhs: Self) -> Self {
        if self.sections.is_empty() {
            return rhs;
        }

        let mut sections_to_merge = VecDeque::from_iter(rhs.sections);
        let sections = &mut self.sections;

        merge_generated_verbatim_section_if_there_is_only_releases_on_lhs(
            &mut sections_to_merge,
            sections,
        );

        let (first_release_pos, first_release_indentation, first_version_prefix) =
            match sections.iter().enumerate().find_map(|(idx, s)| match s {
                Section::Release {
                    heading_level,
                    version_prefix,
                    ..
                } => Some((idx, heading_level, version_prefix)),
                _ => None,
            }) {
                Some((idx, level, prefix)) => (idx, *level, prefix.to_owned()),
                None => {
                    sections.extend(sections_to_merge);
                    return self;
                }
            };

        for mut section_to_merge in sections_to_merge {
            match section_to_merge {
                Section::Verbatim { .. } => {
                    unreachable!(
                        "BUG: generated logs may only have verbatim sections at the beginning"
                    )
                }
                Section::Release { ref name, .. } => {
                    match find_target_section(name, sections, first_release_pos) {
                        Insertion::MergeWith(pos) => sections[pos].merge(section_to_merge),
                        Insertion::At(pos) => {
                            if let Section::Release {
                                heading_level,
                                version_prefix,
                                ..
                            } = &mut section_to_merge
                            {
                                *heading_level = first_release_indentation;
                                *version_prefix = first_version_prefix.clone();
                            }
                            sections.insert(pos, section_to_merge);
                        }
                    }
                }
            }
        }

        self
    }
}

impl Section {
    pub fn merge(&mut self, src: Section) {
        let dest = self;
        match (dest, src) {
            (Section::Verbatim { .. }, _) | (_, Section::Verbatim { .. }) => {
                unreachable!("BUG: we should never try to merge into or from a verbatim section")
            }
            (
                Section::Release {
                    date: dest_date,
                    segments: dest_segments,
                    ..
                },
                Section::Release {
                    date: src_date,
                    segments: src_segments,
                    unknown: src_unknown,
                    ..
                },
            ) => {
                assert!(
                    src_unknown.is_empty(),
                    "shouldn't ever generate 'unknown' portions"
                );
                let has_no_read_only_segments = !dest_segments.iter().any(|s| s.is_read_only());
                let mode = if has_no_read_only_segments {
                    ReplaceMode::ReplaceAllOrAppend
                } else {
                    ReplaceMode::ReplaceAllOrAppendIfPresentInLhs
                };
                for rhs_segment in src_segments {
                    match rhs_segment {
                        Segment::User { .. } => {
                            unreachable!("BUG: User segments are never auto-generated")
                        }
                        Segment::Details(section::Data::Parsed)
                        | Segment::Statistics(section::Data::Parsed)
                        | Segment::Clippy(section::Data::Parsed) => {
                            unreachable!("BUG: Clippy, statistics, and details are set if generated, or not present")
                        }
                        clippy @ Segment::Clippy(_) => merge_read_only_segment(
                            dest_segments,
                            |s| matches!(s, Segment::Clippy(_)),
                            clippy,
                            mode,
                        ),
                        stats @ Segment::Statistics(_) => merge_read_only_segment(
                            dest_segments,
                            |s| matches!(s, Segment::Statistics(_)),
                            stats,
                            mode,
                        ),
                        details @ Segment::Details(_) => merge_read_only_segment(
                            dest_segments,
                            |s| matches!(s, Segment::Details(_)),
                            details,
                            mode,
                        ),
                    }
                }
                *dest_date = src_date;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ReplaceMode {
    ReplaceAllOrAppend,
    ReplaceAllOrAppendIfPresentInLhs,
}

fn merge_read_only_segment(
    dest: &mut Vec<Segment>,
    mut filter: impl FnMut(&section::Segment) -> bool,
    insert: Segment,
    mode: ReplaceMode,
) {
    let mut found_one = false;
    for dest_segment in dest.iter_mut().filter(|s| filter(s)) {
        *dest_segment = insert.clone();
        found_one = true;
    }
    if !found_one && matches!(mode, ReplaceMode::ReplaceAllOrAppend) {
        dest.push(insert);
    }
}

enum Insertion {
    MergeWith(usize),
    At(usize),
}

fn find_target_section(
    wanted: &Version,
    sections: &[Section],
    first_release_index: usize,
) -> Insertion {
    if sections.is_empty() {
        return Insertion::At(0);
    }

    match sections.iter().enumerate().find_map(|(idx, s)| match s {
        Section::Release { name, .. } if name == wanted => Some(Insertion::MergeWith(idx)),
        _ => None,
    }) {
        Some(res) => res,
        None => match wanted {
            Version::Unreleased => Insertion::At(first_release_index),
            Version::Semantic(version) => {
                let (mut pos, min_distance) = sections
                    .iter()
                    .enumerate()
                    .map(|(idx, section)| {
                        (
                            idx,
                            match section {
                                Section::Verbatim { .. } => MAX_DISTANCE,
                                Section::Release { name, .. } => version_distance(name, version),
                            },
                        )
                    })
                    .fold(
                        (usize::MAX, MAX_DISTANCE),
                        |(mut pos, mut dist), (cur_pos, cur_dist)| {
                            if abs_distance(cur_dist) < abs_distance(dist) {
                                dist = cur_dist;
                                pos = cur_pos;
                            }
                            (pos, dist)
                        },
                    );
                if pos == usize::MAX {
                    // We had nothing to compare against, append to the end
                    pos = sections.len();
                }
                if min_distance < (0, 0, 0) {
                    Insertion::At(pos + 1)
                } else {
                    Insertion::At(pos)
                }
            }
        },
    }
}

type Distance = (i64, i64, i64);

const MAX_DISTANCE: Distance = (i64::MAX, i64::MAX, i64::MAX);

fn abs_distance((x, y, z): Distance) -> Distance {
    (x.abs(), y.abs(), z.abs())
}

fn version_distance(from: &Version, to: &semver::Version) -> Distance {
    match from {
        Version::Unreleased => MAX_DISTANCE,
        Version::Semantic(from) => (
            to.major as i64 - from.major as i64,
            to.minor as i64 - from.minor as i64,
            to.patch as i64 - from.patch as i64,
        ),
    }
}

fn merge_generated_verbatim_section_if_there_is_only_releases_on_lhs(
    sections_to_merge: &mut VecDeque<Section>,
    sections: &mut Vec<Section>,
) {
    while let Some(section_to_merge) = sections_to_merge.pop_front() {
        match section_to_merge {
            Section::Verbatim { generated, .. } => {
                assert!(generated, "BUG: rhs must always be generated");
                let first_section = &sections[0];
                if matches!(first_section, Section::Release { .. })
                    || matches!(first_section, Section::Verbatim {generated, ..} if *generated )
                {
                    sections.insert(0, section_to_merge)
                }
            }
            Section::Release { .. } => {
                sections_to_merge.push_front(section_to_merge);
                break;
            }
        }
    }
}
