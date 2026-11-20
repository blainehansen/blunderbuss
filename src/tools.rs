use std::collections::HashMap;

use crate::{JsonResult, utils::{self, PathEnv, ResultExt}};

pub async fn run_tool(
	tool_call: &genai::chat::ToolCall,
	check_command: Option<&str>,
	shell_tools: &HashMap<String, ShellTool>,
	path_env: &utils::PathEnv,
) -> serde_json::Value {
	let raw_args = tool_call.fn_arguments.clone();

	match tool_call.fn_name.as_str() {
		"grep" => {
			run_serde_tool(
				raw_args, path_env, run_grep,
				|a| print!("grep(dir={}, pattern={})", a.dir.display(), a.pattern),
			).await
		},
		"read_file" => {
			run_serde_tool(
				raw_args, path_env, run_read_file,
				|a| print!("read_file(file={})", a.file.display()),
			).await
		},
		"write_file" => {
			run_serde_tool(
				raw_args, path_env, run_write_file,
				|a| print!("write_file(file={}, contents(len)={})", a.file.display(), a.contents.len()),
			).await
		},
		"replace_span" => {
			run_serde_tool(
				raw_args, path_env, run_replace_span,
				|a| print!("replace_span(file={}, prev_span(len)={}, next_span(len)={})", a.file.display(), a.prev_span.len(), a.next_span.len()),
			).await
		},
		"replace_section" => {
			run_serde_tool(
				raw_args, path_env, run_replace_section,
				|a| print!("replace_section(file={}, begin_anchor(len)={}, replacement_text(len)={}, end_anchor(len)={})", a.file.display(), a.begin_anchor.len(), a.replacement_text.len(), a.end_anchor.len()),
			).await
		},
		shell_tool if shell_tools.contains_key(shell_tool) => {
			let shell_tool = shell_tools.get(shell_tool).unwrap();
			run_serde_tool(
				raw_args, path_env, |_, a| run_general_shell(shell_tool, a),
				|a| print!("(shell tool) {} {} {}", shell_tool.first, shell_tool.rest.join(" "), a.suffix.as_deref().unwrap_or("")),
			).await
		},
		"check_command" if check_command.is_some() => {
			let check_command = check_command.unwrap();
			print!("(check_command) `{check_command}`");
			run_check_command(check_command).await
				.inspect(|_| println!(" ✔️")).inspect_err(|_| println!(" ✖️"))
				.map_err(utils::json_err)
				.into_ok_or_err()
		},
		unknown_tool => serde_json::json!({ "error": format!("unknown tool: {unknown_tool}") }),
	}
}

async fn run_serde_tool<'e, T: for<'a> serde::Deserialize<'a>, O: Future<Output=JsonResult>>(
	raw_args: serde_json::Value, path_env: &'e utils::PathEnv,
	run_fn: impl FnOnce(&'e PathEnv, T) -> O, inspect_fn: impl Fn(&'_ T) -> (),
) -> serde_json::Value {
	use async_result_ext::AsyncResultExt;
	serde_json::from_value::<T>(raw_args).map_err(utils::json_err)
		.inspect(inspect_fn)
		.async_map(async |a|
			run_fn(path_env, a).await
			.inspect(|_| println!(" ✔️")).inspect_err(|_| println!(" ✖️"))
			.map_err(utils::json_err).into_ok_or_err()
		).await
		.into_ok_or_err()
}

#[derive(Debug, serde::Deserialize)]
struct ShellCommandArgs {
	suffix: Option<String>,
}

#[derive(Debug)]
pub struct ShellTool {
	first: String,
	rest: Vec<String>,
}

impl ShellTool {
	fn from_vec(shell_split: Vec<String>) -> Option<ShellTool> {
		let mut iter = shell_split.into_iter();
		let first = iter.next()?;
		let rest = iter.collect();
		Some(ShellTool { first, rest })
	}
}

pub fn tool_general_shell(shell_prefix: String) -> Result<((String, ShellTool), genai::chat::Tool), String> {
	let shell_prefix_split = shlex::split(&shell_prefix)
		.ok_or_else(|| format!("{shell_prefix} isn't valid, it couldn't be split by shlex"))?;
	let shell_tool = ShellTool::from_vec(shell_prefix_split)
		.ok_or_else(|| format!("{shell_prefix} is empty after splitting"))?;

	let re = regex::Regex::new(r"[^a-zA-Z0-9_-]+").unwrap();
	let tool_name = re.replace_all(&shell_prefix, "_").into_owned();

	let tool = genai::chat::Tool::new(&tool_name)
		.with_description(format!("run the {shell_prefix} shell command, with possible extra arguments/text appended"))
		.with_schema(serde_json::json!({
			"type": "object",
			"properties": {
				"suffix": {
					"type": "string",
					"description": format!("the extra arguments/text to append to {shell_prefix}. this will be separated from the prefix by a space"),
				 }
			},
		}));

	Ok(((tool_name, shell_tool), tool))
}
async fn run_general_shell(shell_tool: &ShellTool, ShellCommandArgs { suffix }: ShellCommandArgs) -> JsonResult {
	let mut cmd = tokio::process::Command::new(&shell_tool.first);
	cmd.args(&shell_tool.rest);

	if let Some(suffix) = &suffix {
		let command_split = shlex::split(suffix)
			.ok_or_else(|| format!("{suffix} isn't valid, it couldn't be split by shlex"))?;

		cmd.args(command_split);
	}

	let make_full_command = || format!("{} {} {}", shell_tool.first, shell_tool.rest.join(" "), suffix.as_deref().unwrap_or(""));

	let output = cmd.output().await.map_err(|e| {
		let full_command = make_full_command();
		format!("wasn't able to call {full_command}: {e}")
	})?;
	if !output.status.success() {
		let stdout = String::from_utf8_lossy(&output.stdout);
		let stderr = String::from_utf8_lossy(&output.stderr);
		let full_command = make_full_command();
		Err(format!("the command {full_command} failed.\nstdout:\n{stdout}\n\n\nstderr:\n{stderr}"))
	}
	else { Ok(serde_json::json!(String::from_utf8_lossy(&output.stdout))) }
}

pub fn tool_check_command(check_command: &str) -> genai::chat::Tool {
	genai::chat::Tool::new("check_command")
		.with_description(format!("run the {check_command} command that the user specified must pass before this task is complete"))
		// .with_schema(json!({
		// 	"type": "object",
		// 	"properties": {},
		// }))
}
async fn run_check_command(check_command: &str) -> JsonResult {
	let output = tokio::process::Command::new("sh").arg("-c").arg(check_command)
		.output().await
		.map_err(|e| format!("wasn't able to call sh -c '{check_command}': {e}"))?;

	if !output.status.success() {
		let stdout = String::from_utf8_lossy(&output.stdout);
		let stderr = String::from_utf8_lossy(&output.stderr);
		Err(format!("the check command {check_command} failed.\nstdout:\n{stdout}\n\n\nstderr:\n{stderr}"))
	}
	else { Ok(serde_json::json!("check_command successful")) }
}

fn remove_schema_title(schema: &mut serde_json::Value) -> Option<()> {
	schema.as_object_mut()?.remove("title");
	Some(())
}

// #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
// struct Tree {
// 	#[serde(default = "default_dot")]
// 	dir: String,
// }
// pub fn tool_tree() -> genai::chat::Tool {
// 	let schema = schemars::schema_for!(Tree).to_value();

// 	genai::chat::Tool::new("tree").with_schema(schema)
// }
// use ignore::WalkBuilder;
// for result in WalkBuilder::new("./").hidden(false).build() {
// 	println!("{:?}", result);
// }

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GrepArgs {
	#[serde(default = "utils::default_dot")]
	/// the dir to grep through, defaults to `.`
	dir: std::path::PathBuf,
	/// the pattern to grep for
	pattern: String,
}
// TODO should just allow a pipe through to raw grep itself, with some sanitization to ensure the allow/disallow rules are followed
// we can make this safe by putting the shlexed args it gives us *before* the ones we supply manually, since ripgrep says that the *last* instance of a flag wins
// the only tricky thing is that there are flag versions of pattern... so I suppose instructions will have to be given to ensure it always gives a normal pattern
pub fn tool_grep() -> genai::chat::Tool {
	let mut schema = schemars::schema_for!(GrepArgs).to_value();
	remove_schema_title(&mut schema);

	genai::chat::Tool::new("grep")
		.with_description("grep recursively through `dir` to find `pattern`, using ripgrep")
		.with_schema(schema)
}
async fn run_grep(path_env: &utils::PathEnv, GrepArgs { dir, pattern }: GrepArgs) -> JsonResult {
	path_env.dir_is_allowed(&dir)?;

	const MAX_MATCHES: usize = 200;

	let output = tokio::process::Command::new("rg")
		.args(["--line-number", "--no-heading", "--color", "never", "--max-count", "200", "--", &pattern])
		.arg(&dir.as_os_str())
		.output()
		.await.map_err(|e| format!("failed to call rg with {} {}: {e}", &pattern, &dir.display()))?;

	// rg/grep exits with status 1 when there are simply no matches
	if output.status.code() == Some(1) {
		return Err(format!("no matches found for {} {}", &pattern, &dir.display()))
	}

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		return Err(format!("rg failed with status {}:\nstderr:\n{stderr}\nstdout:\n{stdout}", output.status))
	}

	let stdout = String::from_utf8_lossy(&output.stdout);
	let matches: Vec<serde_json::Value> = stdout
		.lines()
		.take(MAX_MATCHES)
		.filter_map(parse_grep_line)
		.collect();

	return Ok(serde_json::json!({ "matches": matches }))
}
fn parse_grep_line(line: &str) -> Option<serde_json::Value> {
	let mut parts = line.splitn(3, ':');
	let path = parts.next()?;
	let line = parts.next()?.parse::<usize>().ok()?;
	let text = parts.next().unwrap_or("").trim();

	Some(serde_json::json!({ "path": path, "line": line, "text": text }))
}


#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ReadFileArgs {
	/// the path of the file to read
	file: std::path::PathBuf,
	// /// an optional chunk of text from which to begin reading the file, only to conserve space if deemed necessary
	// /// must match uniquely
	// starting_from: Option<String>,
	// /// an optional chunk of text at which to end reading the file, only to conserve space if deemed necessary
	// /// must match uniquely
	// ending_at: Option<String>,
}
pub fn tool_read_file() -> genai::chat::Tool {
	let mut schema = schemars::schema_for!(ReadFileArgs).to_value();
	remove_schema_title(&mut schema);

	genai::chat::Tool::new("read_file")
		.with_description("read the contents of `file`")
		// maybe using `starting_from` or `ending_at` to limit how much is read
		.with_schema(schema)
}
async fn run_read_file(path_env: &utils::PathEnv, ReadFileArgs { file }: ReadFileArgs) -> JsonResult {
	path_env.file_is_allowed(&file)?;

	let contents = tokio::fs::read_to_string(&file)
		.await.map_err(|e| format!("unable to read file {}: {e}", file.display()))?;

	Ok(serde_json::json!(contents))
}


#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
/// write `contents` to `file`, creating it if necessary
struct WriteFileArgs {
	/// the path of the file, will be created if it doesn't already exist
	file: std::path::PathBuf,
	/// the contents to write to the file
	contents: String,
}
pub fn tool_write_file() -> genai::chat::Tool {
	let mut schema = schemars::schema_for!(WriteFileArgs).to_value();
	remove_schema_title(&mut schema);
	genai::chat::Tool::new("write_file")
		.with_schema(schema)
}
async fn run_write_file(path_env: &utils::PathEnv, WriteFileArgs { file, contents }: WriteFileArgs) -> JsonResult {
	path_env.file_is_allowed(&file)?;

	// TODO figure out whether it matters that this parent might in fact be the parent of cwd
	if let Some(parent) = file.parent() {
		tokio::fs::create_dir_all(parent)
			.await.map_err(|e| format!("unable to create parent directories for {}: {e}", file.display()))?;
	}

	use tokio::io::AsyncWriteExt;
	tokio::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&file)
		.await.map_err(|e| format!("unable to open file {}: {e}", file.display()))?
		.write_all(contents.as_bytes())
		.await.map_err(|e| format!("unable to write to {}: {e}", file.display()))?;

	Ok(serde_json::json!(format!("write_file successful")))
}


#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ReplaceSpanArgs {
	/// the path of the file
	file: std::path::PathBuf,
	/// the text that will be replaced, including whitespace
	prev_span: String,
	/// the text doing the replacing, including whitespace
	next_span: String,
}
pub fn tool_replace_span() -> genai::chat::Tool {
	let mut schema = schemars::schema_for!(ReplaceSpanArgs).to_value();
	remove_schema_title(&mut schema);
	genai::chat::Tool::new("replace_span")
		.with_description("in `file` replace all instances of `prev_span` with `next_span`")
		.with_schema(schema)
}
async fn run_replace_span(path_env: &utils::PathEnv, ReplaceSpanArgs { file, prev_span, next_span }: ReplaceSpanArgs) -> JsonResult {
	path_env.file_is_allowed(&file)?;

	if prev_span.is_empty() {
		return Err(format!("prev_span must not be empty"));
	}

	let content = tokio::fs::read_to_string(&file)
		.await.map_err(|e| format!("io error while reading {}: {e}", file.display()))?;

	if content.find(&prev_span).is_none() {
		return Err(format!("prev_span argument not found in {}", file.display()));
	}

	let new_content = content.replace(&prev_span, &next_span);
	tokio::fs::write(&file, new_content)
		.await.map_err(|e| format!("io error while reading {}: {e}", file.display()))?;

	Ok(serde_json::json!(format!("replace_span successful")))
}


#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
/// replace a section of text in a file, marked by anchors immediately before and after it
struct ReplaceSectionArgs {
	/// the path of the file
	file: std::path::PathBuf,
	/// the text that comes immediately before the text to be replaced, which must match precisely including whitespace
	/// this text will remain in the file
	/// this text must be unique in the file
	begin_anchor: String,
	/// the text that should be written in the file starting after begin_anchor
	replacement_text: String,
	/// the text that comes immediately after the text to be replaced, which must match precisely including whitespace
	/// this text will remain in the file
	/// this text doesn't have to be unique in the file, the first match after begin_anchor will be used
	end_anchor: String,
}
pub fn tool_replace_section() -> genai::chat::Tool {
	let mut schema = schemars::schema_for!(ReplaceSectionArgs).to_value();
	remove_schema_title(&mut schema);
	genai::chat::Tool::new("replace_section")
		.with_schema(schema)
}
async fn run_replace_section(
	path_env: &utils::PathEnv,
	ReplaceSectionArgs { file, begin_anchor, replacement_text, end_anchor }: ReplaceSectionArgs,
) -> JsonResult {
	path_env.file_is_allowed(&file)?;

	if begin_anchor.is_empty() || end_anchor.is_empty() {
		return Err(format!("begin_anchor and end_anchor must not be empty"));
	}

	let content = tokio::fs::read_to_string(&file)
		.await.map_err(|e| format!("io error while reading {}: {e}", file.display()))?;

	let new_content = do_replace_section(&content, &begin_anchor, &replacement_text, &end_anchor)
		.map_err(|e| match e {
			ReplaceSectionError::BeginNotFound => format!("begin_anchor argument not found in {}", file.display()),
			ReplaceSectionError::BeginNotUnique => format!("begin_anchor is found multiple times in {}", file.display()),
			ReplaceSectionError::EndNotFound => format!("end_anchor argument not found in {}", file.display()),
		})?;

	tokio::fs::write(&file, new_content)
		.await.map_err(|e| format!("io error while reading {}: {e}", file.display()))?;

	Ok(serde_json::json!(format!("replace_section successful")))
}

#[derive(Debug)]
enum ReplaceSectionError {
	BeginNotFound,
	BeginNotUnique,
	EndNotFound,
}

fn do_replace_section(
	content: &str,
	begin_anchor: &str, replacement_text: &str, end_anchor: &str,
) -> Result<String, ReplaceSectionError> {
	let mut new_content = String::new();
	let mut cursor = utils::StrCursor::new(&content);

	// add everything up to and including begin_anchor
	new_content.push_str(
		cursor.advance_past(&begin_anchor).ok_or(ReplaceSectionError::BeginNotFound)?
	);
	// ensure begin_anchor is unique
	if cursor.remaining().find(&begin_anchor).is_some() {
		return Err(ReplaceSectionError::BeginNotUnique);
	}
	// throw away the stuff up to end_anchor
	let _ = cursor.advance_to(&end_anchor).ok_or(ReplaceSectionError::EndNotFound)?;
	// add the actual new target text
	new_content.push_str(&replacement_text);
	// and add everything else after that
	new_content.push_str(cursor.into_remaining());

	Ok(new_content)
}

#[test]
fn test_replace_section() {
	let content = "aaa bbb stuff to replace bbb ccc";
	let begin_anchor = "aaa bbb ";
	let replacement_text = "now this";
	let end_anchor = " bbb ccc";
	assert_eq!(
		do_replace_section(content, begin_anchor, replacement_text, end_anchor).unwrap(),
		"aaa bbb now this bbb ccc",
	);

	let content = "aaa
	bbb
		stuff to replace
all these things
		aren't good trying to

					get rid of them
	bbb ccc
";
	let begin_anchor = "aaa
	bbb
";
	let replacement_text = "	aligned
stuff  that
			is much more
	reasonable
";
	let end_anchor = "	bbb ccc
";
	let expected = "aaa
	bbb
	aligned
stuff  that
			is much more
	reasonable
	bbb ccc
";
	assert_eq!(do_replace_section(content, begin_anchor, replacement_text, end_anchor).unwrap(), expected);
}
