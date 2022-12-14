use std::{
    convert::TryFrom,
    iter::{FromIterator, Peekable},
    ops::Range,
    str::FromStr,
};

use git_repository::bstr::ByteSlice;
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_till, take_while, take_while_m_n},
    combinator::{all_consuming, map, map_res, opt},
    error::{FromExternalError, ParseError},
    sequence::{delimited, preceded, separated_pair, terminated, tuple},
    Finish, IResult,
};
use pulldown_cmark::{Event, HeadingLevel, OffsetIter, Tag};

use crate::{
    changelog,
    changelog::{section, section::Segment, Section},
    ChangeLog,
};

impl ChangeLog {
    /// Obtain as much information as possible from `input` and keep everything we didn't understand in respective sections.
    pub fn from_markdown(input: &str) -> ChangeLog {
        let mut sections = Vec::new();
        let mut section_body = String::new();
        let mut previous_headline = None::<Headline>;
        let mut first_heading_level = None;
        for line in input.as_bytes().as_bstr().lines_with_terminator() {
            let line = line.to_str().expect("valid UTF-8");
            match Headline::try_from(line) {
                Ok(headline) => {
                    first_heading_level.get_or_insert(headline.level);
                    match previous_headline {
                        Some(mut headline) => {
                            headline.level = first_heading_level.expect("set first");
                            sections.push(Section::from_headline_and_body(
                                headline,
                                std::mem::take(&mut section_body),
                            ));
                        }
                        None => {
                            if !section_body.is_empty() {
                                sections.push(Section::Verbatim {
                                    text: std::mem::take(&mut section_body),
                                    generated: false,
                                })
                            }
                        }
                    };
                    previous_headline = Some(headline);
                }
                Err(()) => {
                    section_body.push_str(line);
                }
            }
        }

        match previous_headline {
            Some(headline) => {
                sections.push(Section::from_headline_and_body(
                    headline,
                    std::mem::take(&mut section_body),
                ));
            }
            None => sections.push(Section::Verbatim {
                text: section_body,
                generated: false,
            }),
        }

        let insert_sorted_at_pos = sections
            .first()
            .map(|s| match s {
                Section::Verbatim { .. } => 1,
                Section::Release { .. } => 0,
            })
            .unwrap_or(0);
        let mut non_release_sections = Vec::new();
        let mut release_sections = Vec::new();
        for section in sections {
            match section {
                Section::Verbatim { .. } => non_release_sections.push(section),
                Section::Release { .. } => release_sections.push(section),
            }
        }
        release_sections.sort_by(|lhs, rhs| match (lhs, rhs) {
            (Section::Release { name: lhs, .. }, Section::Release { name: rhs, .. }) => {
                lhs.cmp(rhs).reverse()
            }
            _ => unreachable!("BUG: there are only release sections here"),
        });
        let mut sections = Vec::from_iter(non_release_sections.drain(..insert_sorted_at_pos));
        sections.append(&mut release_sections);
        sections.append(&mut non_release_sections);
        ChangeLog { sections }
    }
}

impl Section {
    fn from_headline_and_body(
        Headline {
            level,
            version_prefix,
            version,
            date,
        }: Headline,
        body: String,
    ) -> Self {
        let mut events = pulldown_cmark::Parser::new_ext(&body, pulldown_cmark::Options::all())
            .into_offset_iter()
            .peekable();
        let mut unknown = String::new();
        let mut segments = Vec::new();

        let mut unknown_range = None;
        let removed_messages = Vec::new();
        while let Some((e, range)) = events.next() {
            match e {
                Event::Html(text) if text.starts_with(Section::UNKNOWN_TAG_START) => {
                    record_unknown_range(&mut segments, unknown_range.take(), &body);
                    for (event, _range) in events.by_ref().take_while(
                        |(e, _range)| !matches!(e, Event::Html(text) if text.starts_with(Section::UNKNOWN_TAG_END)),
                    ) {
                        track_unknown_event(event, &mut unknown);
                    }
                }
                Event::Start(Tag::Heading(indent, _, _)) => {
                    record_unknown_range(&mut segments, unknown_range.take(), &body);
                    enum State {
                        SkipGenerated,
                        ConsiderUserAuthored,
                    }
                    let state = match events.next() {
                        Some((Event::Text(title), _range))
                            if title.starts_with(section::segment::ThanksClippy::TITLE) =>
                        {
                            segments.push(Segment::Clippy(section::Data::Parsed));
                            State::SkipGenerated
                        }
                        Some((Event::Text(title), _range))
                            if title.starts_with(section::segment::CommitStatistics::TITLE) =>
                        {
                            segments.push(Segment::Statistics(section::Data::Parsed));
                            State::SkipGenerated
                        }
                        Some((Event::Text(title), _range))
                            if title.starts_with(section::segment::Details::TITLE) =>
                        {
                            segments.push(Segment::Details(section::Data::Parsed));
                            State::SkipGenerated
                        }
                        Some((_event, next_range)) => {
                            update_unknown_range(&mut unknown_range, range);
                            update_unknown_range(&mut unknown_range, next_range);
                            State::ConsiderUserAuthored
                        }
                        None => State::ConsiderUserAuthored,
                    };

                    events
                        .by_ref()
                        .take_while(|(e, range)| {
                            if matches!(state, State::ConsiderUserAuthored) {
                                update_unknown_range(&mut unknown_range, range.clone());
                            }
                            !matches!(e, Event::End(Tag::Heading(_, _, _)))
                        })
                        .count();
                    match state {
                        State::SkipGenerated => {
                            skip_to_next_section_title(&mut events, indent);
                        }
                        State::ConsiderUserAuthored => {}
                    }
                }
                _unknown_event => update_unknown_range(&mut unknown_range, range),
            };
        }
        record_unknown_range(&mut segments, unknown_range.take(), &body);
        Section::Release {
            name: match version {
                Some(version) => changelog::Version::Semantic(version),
                None => changelog::Version::Unreleased,
            },
            version_prefix,
            date,
            removed_messages,
            heading_level: level,
            segments,
            unknown,
        }
    }
}

fn update_unknown_range(target: &mut Option<Range<usize>>, source: Range<usize>) {
    match target {
        Some(range_thus_far) => {
            if source.end > range_thus_far.end {
                range_thus_far.end = source.end;
            }
        }
        None => *target = source.into(),
    }
}

fn record_unknown_range(
    out: &mut Vec<section::Segment>,
    range: Option<Range<usize>>,
    markdown: &str,
) {
    if let Some(range) = range {
        out.push(Segment::User {
            markdown: markdown[range].to_owned(),
        })
    }
}

fn track_unknown_event(unknown_event: Event<'_>, unknown: &mut String) {
    log::trace!("Cannot handle {:?}", unknown_event);
    match unknown_event {
        Event::Html(text)
        | Event::Code(text)
        | Event::Text(text)
        | Event::FootnoteReference(text)
        | Event::Start(Tag::FootnoteDefinition(text))
        | Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(text)))
        | Event::Start(Tag::Link(_, text, _))
        | Event::Start(Tag::Image(_, text, _)) => unknown.push_str(text.as_ref()),
        _ => {}
    }
}

fn skip_to_next_section_title(events: &mut Peekable<OffsetIter<'_, '_>>, level: HeadingLevel) {
    while let Some((event, _range)) = events.peek() {
        match event {
            Event::Start(Tag::Heading(indent, _, _)) if *indent == level => break,
            _ => {
                events.next();
                continue;
            }
        }
    }
}

struct Headline {
    level: usize,
    version_prefix: String,
    version: Option<semver::Version>,
    date: Option<time::OffsetDateTime>,
}

impl<'a> TryFrom<&'a str> for Headline {
    type Error = ();

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        all_consuming(headline::<()>)(value)
            .finish()
            .map(|(_, h)| h)
    }
}

fn headline<'a, E: ParseError<&'a str> + FromExternalError<&'a str, ()>>(
    i: &'a str,
) -> IResult<&'a str, Headline, E> {
    let hashes = take_while(|c: char| c == '#');
    let greedy_whitespace = |i| take_while(|c: char| c.is_whitespace())(i);
    let take_n_digits = |n: usize| {
        map_res(take_while_m_n(n, n, |c: char| c.is_ascii_digit()), |num| {
            u32::from_str(num).map_err(|_| ())
        })
    };
    map(
        terminated(
            tuple((
                separated_pair(
                    hashes,
                    greedy_whitespace,
                    alt((
                        tuple((
                            opt(tag("v")),
                            map_res(take_till(|c: char| c.is_whitespace()), |v| {
                                semver::Version::parse(v).map_err(|_| ()).map(Some)
                            }),
                        )),
                        map(tag_no_case("unreleased"), |_| (None, None)),
                    )),
                ),
                opt(preceded(
                    greedy_whitespace,
                    delimited(
                        tag("("),
                        map_res(
                            tuple((
                                take_n_digits(4),
                                tag("-"),
                                take_n_digits(2),
                                tag("-"),
                                take_n_digits(2),
                            )),
                            |(year, _, month, _, day)| {
                                time::Month::try_from(month as u8).map_err(|_| ()).and_then(
                                    |month| {
                                        time::Date::from_calendar_date(
                                            year as i32,
                                            month,
                                            day as u8,
                                        )
                                        .map_err(|_| ())
                                        .map(|d| d.midnight().assume_utc())
                                    },
                                )
                            },
                        ),
                        tag(")"),
                    ),
                )),
            )),
            greedy_whitespace,
        ),
        |((hashes, (prefix, version)), date)| Headline {
            level: hashes.len(),
            version_prefix: prefix.map(ToOwned::to_owned).unwrap_or_else(String::new),
            version,
            date,
        },
    )(i)
}
