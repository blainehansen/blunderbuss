use nom::{
	IResult, Parser,
	branch::alt,
	bytes::complete::{tag, take_while},
	combinator::{all_consuming, value},

};
use nom_language::error::{VerboseError, convert_error};

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
	pub tools: Option<Vec<String>>,
	pub model: Option<String>,
	pub allow: Option<Vec<PermGlob>>,
	pub disallow: Option<Vec<PermGlob>>,
	pub maxturns: Option<usize>,
	pub check: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PermGlob {
	pub kind: PermKind,
	pub glob: String,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum PermKind {
	Read,
	Write,
}

pub fn parse_instructions(input: &str) -> anyhow::Result<(Params, String)> {
	let mut matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
	matter.excerpt_delimiter = None;

	let parsed = matter.parse(input)?;
	// TODO strip out the html comments?
	Ok((parsed.data.unwrap_or_default(), parsed.content))
}

fn not_newline(c: char) -> bool {
	c != '\n' && c != '\r'
}

fn perm_glob(input: &str) -> IResult<&str, PermGlob, VerboseError<&str>> {
	let (input, kind) = alt((
		value(PermKind::Read, tag("read ")),
		value(PermKind::Write, tag("write ")),
	)).parse(input)?;

	let (input, body) = take_while(not_newline).parse(input)?;
	Ok((input, PermGlob { kind, glob: body.to_owned() }))
}


impl<'de> serde::Deserialize<'de> for PermGlob {
	fn deserialize<D>(deserializer: D) -> Result<PermGlob, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		struct PermGlobVisitor;

		impl<'de> serde::de::Visitor<'de> for PermGlobVisitor {
			type Value = PermGlob;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("read|write <file glob>")
			}

			fn visit_str<E>(self, value: &str) -> Result<PermGlob, E>
			where
				E: serde::de::Error,
			{
				let (_remaining, perm_glob) = all_consuming(perm_glob).parse(value)
					.map_err(|e| {
						match e {
							nom::Err::Incomplete(_) => E::custom(format!("incomplete input: {value}")),
							nom::Err::Error(e) | nom::Err::Failure(e) => E::custom(convert_error(value, e)),
						}
					})?;

				Ok(perm_glob)
			}
		}

		deserializer.deserialize_str(PermGlobVisitor)
	}
}


#[test]
fn basic_examples() {
	let input = "";

	let (raw_params, body) = parse_instructions(input).unwrap();
	assert!(raw_params.tools.is_none());
	assert!(raw_params.model.is_none());
	assert!(raw_params.allow.is_none());
	assert!(raw_params.disallow.is_none());
	assert!(raw_params.maxturns.is_none());
	assert!(raw_params.check.is_none());
	assert_eq!(body, "");

	let input = r#"---
# yaml comment
maxturns: 5
# maxturns: 40
model: gemma
# model: kimi
tools:
- ls -al
# - git commit
- git ls
allow:
- write .env.example
disallow:
- read *.env
check: cargo check
# this comment is stripped
# also this
---

So much to do

fix all the problems and make me happy
---
some: misleading things
---"#;

	let (raw_params, body) = parse_instructions(input).unwrap();

	assert_eq!(raw_params.tools.unwrap(), vec!["ls -al", "git ls"]);
	assert_eq!(raw_params.model.as_deref(), Some("gemma"));
	assert_eq!(raw_params.allow.unwrap(), vec![PermGlob { kind: PermKind::Write, glob: ".env.example".into() }]);
	assert_eq!(raw_params.disallow.unwrap(), vec![PermGlob { kind: PermKind::Read, glob: "*.env".into() }]);
	assert_eq!(raw_params.maxturns, Some(5));
	assert_eq!(raw_params.check.as_deref(), Some("cargo check"));

	assert_eq!(body, "So much to do

fix all the problems and make me happy
---
some: misleading things
---");
}
