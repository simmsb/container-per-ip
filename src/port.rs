use std::fmt::Display;
use std::ops::{Range, RangeInclusive};

use chumsky::primitive::{choice, end, just};
use chumsky::{text, Parser};
use itertools::Itertools;
use miette::SourceSpan;

#[derive(Debug)]
pub struct SepString(Vec<String>);

impl Display for SepString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().format("\n").fmt(f)
    }
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("Couldn't parse port config")]
pub enum Error {
    #[error("Port range doesn't make sense")]
    BadRange {
        #[source_code]
        src: String,

        #[label("This should be less than")]
        a_span: SourceSpan,

        #[label("this")]
        b_span: SourceSpan,

        #[help]
        advice: SepString,

        #[related]
        related: Vec<Self>,
    },

    #[error("Remap ranges must be the same size")]
    BadRemapRange {
        #[source_code]
        src: String,

        #[label("This range should be as long as")]
        a_span: SourceSpan,

        #[label("this")]
        b_span: SourceSpan,

        #[help]
        advice: SepString,

        #[related]
        related: Vec<Self>,
    },

    #[error("Unexpected input: {}", unexpected)]
    UnexpectedInput {
        #[source_code]
        src: String,

        #[label("{expected_msg}")]
        err_span: SourceSpan,

        expected_msg: String,

        unexpected: String,

        #[help]
        advice: SepString,

        #[related]
        related: Vec<Self>,
    },
}

impl Error {
    fn finalize(self) -> Self {
        let help = r#"
The supported syntax for ports is:
  udp:53
  tcp:8080:80             (outside:inside)
  tcp:5000-5100           (range)
  tcp:5000-5100:6000-6100 (outside range - inside range)
"#;

        match self {
            Error::BadRange {
                src,
                a_span,
                b_span,
                mut advice,
                related,
            } => {
                advice.0.push(help.to_owned());
                Error::BadRange {
                    src,
                    a_span,
                    b_span,
                    advice,
                    related,
                }
            }
            Error::BadRemapRange {
                src,
                a_span,
                b_span,
                mut advice,
                related,
            } => {
                advice.0.push(help.to_owned());
                Error::BadRemapRange {
                    src,
                    a_span,
                    b_span,
                    advice,
                    related,
                }
            }
            Error::UnexpectedInput {
                src,
                err_span,
                expected_msg,
                unexpected,
                mut advice,
                related,
            } => {
                advice.0.push(help.to_owned());
                Error::UnexpectedInput {
                    src,
                    err_span,
                    expected_msg,
                    unexpected,
                    advice,
                    related,
                }
            }
        }
    }
}

#[derive(Debug)]
enum ParsePortError {
    BadRange {
        a_span: SourceSpan,
        b_span: SourceSpan,
        advice: Vec<String>,
        related: Vec<Self>,
    },
    BadRemapRange {
        a_span: SourceSpan,
        b_span: SourceSpan,
        advice: Vec<String>,
        related: Vec<Self>,
    },
    UnexpectedInput {
        err_span: SourceSpan,
        expected_msg: String,
        unexpected: String,
        advice: Vec<String>,
        related: Vec<Self>,
    },
}

impl ParsePortError {
    fn with_source(self, src: String) -> Error {
        match self {
            ParsePortError::BadRange {
                a_span,
                b_span,
                advice,
                related,
            } => Error::BadRange {
                src: src.clone(),
                a_span,
                b_span,
                advice: SepString(advice),
                related: related
                    .into_iter()
                    .map(|e| e.with_source(src.clone()))
                    .collect(),
            },
            ParsePortError::BadRemapRange {
                a_span,
                b_span,
                advice,
                related,
            } => Error::BadRemapRange {
                src: src.clone(),
                a_span,
                b_span,
                advice: SepString(advice),
                related: related
                    .into_iter()
                    .map(|e| e.with_source(src.clone()))
                    .collect(),
            },
            ParsePortError::UnexpectedInput {
                err_span,
                expected_msg,
                unexpected,
                advice,
                related,
            } => Error::UnexpectedInput {
                src: src.clone(),
                err_span,
                expected_msg,
                unexpected,
                advice: SepString(advice),
                related: related
                    .into_iter()
                    .map(|e| e.with_source(src.clone()))
                    .collect(),
            },
        }
    }
}

impl chumsky::Error<char> for ParsePortError {
    type Span = Range<usize>;

    type Label = &'static str;

    fn expected_input_found<Iter: IntoIterator<Item = Option<char>>>(
        span: Self::Span,
        expected: Iter,
        found: Option<char>,
    ) -> Self {
        let expected_msg = format!(
            "Expecting: {}",
            expected
                .into_iter()
                .map(|c| c.map(|c| c.to_string()).unwrap_or_else(|| "EOF".to_owned()))
                .format(", ")
        );

        Self::UnexpectedInput {
            err_span: span.into(),
            expected_msg,
            unexpected: found
                .map(|c| c.to_string())
                .unwrap_or_else(|| "EOF".to_owned()),
            advice: vec![],
            related: vec![],
        }
    }

    fn with_label(self, label: Self::Label) -> Self {
        match self {
            ParsePortError::BadRange {
                a_span,
                b_span,
                mut advice,
                related: inner,
            } => {
                advice.push(label.to_owned());
                Self::BadRange {
                    a_span,
                    b_span,
                    advice,
                    related: inner,
                }
            }
            ParsePortError::BadRemapRange {
                a_span,
                b_span,
                mut advice,
                related: inner,
            } => {
                advice.push(label.to_owned());
                Self::BadRemapRange {
                    a_span,
                    b_span,
                    advice,
                    related: inner,
                }
            }
            ParsePortError::UnexpectedInput {
                err_span,
                expected_msg,
                unexpected,
                mut advice,
                related,
            } => {
                advice.push(label.to_owned());
                Self::UnexpectedInput {
                    err_span,
                    expected_msg,
                    unexpected,
                    advice,
                    related,
                }
            }
        }
    }

    fn merge(self, other: Self) -> Self {
        match self {
            ParsePortError::BadRange {
                a_span,
                b_span,
                advice,
                mut related,
            } => {
                related.push(other);

                Self::BadRange {
                    a_span,
                    b_span,
                    advice,
                    related,
                }
            }
            ParsePortError::BadRemapRange {
                a_span,
                b_span,
                advice,
                mut related,
            } => {
                related.push(other);

                Self::BadRemapRange {
                    a_span,
                    b_span,
                    advice,
                    related,
                }
            }
            ParsePortError::UnexpectedInput {
                err_span,
                expected_msg,
                unexpected,
                advice,
                mut related,
            } => {
                related.push(other);

                Self::UnexpectedInput {
                    err_span,
                    expected_msg,
                    unexpected,
                    advice,
                    related,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PortMode {
    Tcp,
    Udp,
}

pub fn parse_port_mapping(i: &str) -> Result<PortConfig, Error> {
    #[rustfmt::skip]
    let mode = choice((
        just("tcp").to(PortMode::Tcp).labelled("tcp"),
        just("udp").to(PortMode::Udp).labelled("udp"),
    ));

    #[rustfmt::skip]
    let number = text::int::<char, _>(10)
        .from_str::<u16>()
        .unwrapped();

    let range = number
        .map_with_span(|a, a_span: Range<usize>| (a, a_span.into()))
        .then(
            just("-")
                .ignore_then(number.map_with_span(|b, b_span: Range<usize>| (b, b_span.into()))),
        )
        .validate(|((a, a_span), (b, b_span)), _span, emit| {
            if a >= b {
                emit(ParsePortError::BadRange {
                    a_span,
                    b_span,
                    advice: vec![],
                    related: vec![],
                })
            }

            RangeInclusive::new(a, b)
        });

    let range_ = range.map(PortMapping::Range);

    let remap = number
        .then(just(":").ignore_then(number))
        .map(|(a, b)| PortMapping::Remap(a, b));

    let remap_range = range
        .map_with_span(|a, a_span: Range<usize>| (a, a_span.into()))
        .then(
            just(":")
                .ignore_then(range.map_with_span(|b, b_span: Range<usize>| (b, b_span.into()))),
        )
        .validate(|((a, a_span), (b, b_span)), _span, emit| {
            if a.len() != b.len() {
                emit(ParsePortError::BadRemapRange {
                    a_span,
                    b_span,
                    advice: vec![],
                    related: vec![],
                })
            }

            let offset = *b.start() as i32 - *a.start() as i32;

            PortMapping::RemapRange(a, offset)
        });

    let single = number.map(PortMapping::Single);

    let p = mode
        .then_ignore(just(":"))
        .then(choice((remap_range, remap, range_, single)))
        .then_ignore(end())
        .map(|(mode, mapping)| PortConfig { mode, mapping });

    match p.parse(i) {
        Ok(x) => Ok(x),
        Err(mut errs) => Err(errs.pop().unwrap().with_source(i.to_owned()).finalize()),
    }
}

#[derive(Debug, Clone)]
pub struct PortConfig {
    pub mode: PortMode,
    pub mapping: PortMapping,
}

impl PortConfig {
    pub fn all_ports(&self) -> Vec<(PortMode, (u16, u16))> {
        self.mapping
            .all_ports()
            .into_iter()
            .map(|x| (self.mode, x))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum PortMapping {
    RemapRange(RangeInclusive<u16>, i32),
    Range(RangeInclusive<u16>),
    Remap(u16, u16),
    Single(u16),
}

impl PortMapping {
    pub fn all_ports(&self) -> Vec<(u16, u16)> {
        match self {
            PortMapping::RemapRange(r, o) => {
                r.clone().map(|x| (x, (x as i32 + o) as u16)).collect()
            }
            PortMapping::Range(r) => r.clone().map(|x| (x, x)).collect(),
            PortMapping::Remap(a, b) => vec![(*a, *b)],
            PortMapping::Single(x) => vec![(*x, *x)],
        }
    }
}
