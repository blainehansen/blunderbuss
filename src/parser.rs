use crate::utils::OptionExt;

use nom::{
	IResult, Parser, branch::alt, bytes::complete::{tag, take_until, take_while}, character::complete::anychar, combinator::{cut, map, map_res, rest, value}, error::context, multi::fold_many0, sequence::{delimited, pair, preceded, terminated},
};
use nom_language::error::{VerboseError, convert_error};

#[derive(Debug, Default)]
pub struct Instructions {
	pub tools: Option<Vec<String>>,
	pub model: Option<String>,
	pub allows: Option<Vec<PermGlob>>,
	pub disallows: Option<Vec<PermGlob>>,
	pub maxturns: Option<usize>,
	pub check_command: Option<String>,
	pub text: String,
}

#[derive(Debug, Copy, Clone)]
pub enum TextKind {
	Model,
	Tool,
	Check,
}

#[derive(Debug)]
pub struct PermGlob {
	pub kind: PermKind,
	pub glob: String,
}

#[derive(Debug, Copy, Clone)]
pub enum PermKind {
	Read,
	Write,
}

fn newline(c: char) -> bool {
	c == '\n' || c == '\r'
}

// anything except `]`, `\n`, `\r`
fn body_char(c: char) -> bool {
	c != ']' && c != '\n' && c != '\r'
}

fn html_comment(input: &str) -> IResult<&str, &str, VerboseError<&str>> {
	preceded(
		tag("<!--"),
		alt((
			terminated(take_until("-->"), tag("-->")),
			rest,
		)),
	).parse(input)
}

fn text_command(input: &str) -> IResult<&str, (TextKind, String), VerboseError<&str>> {
	let (input, text_kind) = alt((
		value(TextKind::Tool, tag("/tool")),
		value(TextKind::Model, tag("/model")),
		value(TextKind::Check, tag("/check")),
	)).parse(input)?;

	let (input, body) = cut(context(
		"expected `[<tool prefix>]` after /tool",
		delimited(tag("["), take_while(body_char), tag("]")),
	)).parse(input)?;
	// TODO have to figure out the whitespace stuff of text commands, they shouldn't be smushed with other things
	let (input, _) = take_while(newline)(input)?;
	Ok((input, (text_kind, body.to_owned())))
}

fn perm_glob(input: &str) -> IResult<&str, (bool, PermGlob), VerboseError<&str>> {
	let (input, is_allow) = alt((
		value(true, tag("/allow")),
		value(false, tag("/disallow")),
	)).parse(input)?;

	let (input, (kind, body)) = cut(context(
		if is_allow { "expected `[<file glob>]` after /allow" } else { "expected `[<file glob>]` after /disallow" },
		pair(
			perm_kind,
			delimited(tag("["), take_while(body_char), tag("]")),
		),
	)).parse(input)?;
	// TODO have to figure out the whitespace stuff of text commands, they shouldn't be smushed with other things
	let (input, _) = take_while(newline)(input)?;
	Ok((input, (is_allow, PermGlob { kind, glob: body.to_owned() })))
}

fn perm_kind(input: &str) -> IResult<&str, PermKind, VerboseError<&str>> {
	let (input, _) = tag("/")(input)?;
	let (input, k) = alt((
		value(PermKind::Read, tag("read")),
		value(PermKind::Write, tag("write")),
	)).parse(input)?;
	Ok((input, k))
}

fn maxturns(input: &str) -> IResult<&str, usize, VerboseError<&str>> {
	let (input, _) = tag("/maxturns")(input)?;
	let (input, m) = cut(context(
		"expected `[<uint>]` after /maxturns",
		map_res(delimited(tag("["), take_while(body_char), tag("]")), |s: &str| s.parse::<usize>()),
	)).parse(input)?;
	// TODO have to figure out the whitespace stuff of text commands, they shouldn't be smushed with other things
	let (input, _) = take_while(newline)(input)?;
	Ok((input, m))
}

enum Segment {
	TextCommand((TextKind, String)),
	PermGlob((bool, PermGlob)),
	MaxTurns(usize),
	Text(char),
	Comment,
}

fn segment(input: &str) -> IResult<&str, Segment, VerboseError<&str>> {
	alt((
		map(text_command, Segment::TextCommand),
		map(perm_glob, Segment::PermGlob),
		map(maxturns, Segment::MaxTurns),
		map(anychar, Segment::Text),
		map(html_comment, |_| Segment::Comment),
	)).parse(input)
}

pub fn parse_instructions(input: &str) -> Result<Instructions, String> {
	match fold_many0(
		segment,
		Instructions::default,
		|mut instructions, seg| {
			match seg {
				Segment::TextCommand((text_kind, body)) => {
					match text_kind {
						TextKind::Tool => instructions.tools.get_or_insert_default().push(body),
						TextKind::Model => instructions.model.try_insert(body).unwrap(),
						TextKind::Check => instructions.check_command.try_insert(body).unwrap(),
					}
				},
				Segment::PermGlob((is_allow, glob)) => {
					if is_allow { instructions.allows.get_or_insert_default().push(glob) }
					else { instructions.disallows.get_or_insert_default().push(glob) }
				},
				Segment::MaxTurns(maxturns) => instructions.maxturns.try_insert(maxturns).unwrap(),
				Segment::Text(c) => instructions.text.push(c),
				Segment::Comment => {},
			}
			instructions
		},
	).parse(input)
	{
		Ok(("", instructions)) => Ok(instructions),
		// unreachable in practice: `segment` succeeds on any non-empty input
		Ok((rest, _)) => Err(format!("parser stopped early at {rest:?}")),
		Err(nom::Err::Incomplete(_)) => Err("incomplete input".into()),
		Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => Err(convert_error(input, e)),
	}
}

#[test]
fn example() {
	let input = "\
/model[gemini::gemma-4-26b-a4b-it]
<!-- /tool[my-tool] -->
/maxturns[5]

/tool[ls]

blah blah random text instructions blah blah
";
	let doc = parse_instructions(input).unwrap();
	assert_eq!(doc.model.as_deref(), Some("gemini::gemma-4-26b-a4b-it"));
	assert_eq!(doc.maxturns, Some(5));
	assert_eq!(doc.tools, Some(vec!["ls".into()]));
	assert_eq!(doc.text, "blah blah random text instructions blah blah\n");
}

#[test]
fn slash_without_bracket_is_plain_text() {
	let doc = parse_instructions("see /foo and friends").unwrap();
	assert!(doc.tools.is_none());
	assert!(doc.model.is_none());
	assert!(doc.allows.is_none());
	assert!(doc.disallows.is_none());
	assert!(doc.maxturns.is_none());
	assert!(doc.check_command.is_none());
	assert_eq!(doc.text, "see /foo and friends");
}

#[test]
fn unclosed_command_is_an_error() {
	let err = parse_instructions("hello\n/tool[abc").unwrap_err();
	assert!(err.contains("expected `]`"), "{err}");
}

#[test]
fn newline_inside_body_is_an_error() {
	let err = parse_instructions("/tool[a\nb] and more").unwrap_err();
	assert!(err.contains("newlines are not allowed"), "{err}");
}

#[test]
fn opening_bracket_commits_even_mid_text() {
	let doc = parse_instructions("go to https://example.com/a[b] now").unwrap();
	assert!(doc.tools.is_none());
	assert!(doc.model.is_none());
	assert!(doc.allows.is_none());
	assert!(doc.disallows.is_none());
	assert!(doc.maxturns.is_none());
	assert!(doc.check_command.is_none());
	assert_eq!(doc.text, "go to https://example.com/a[b] now");
}
