use std::collections::HashMap;

use crate::{JsonResult, utils::{self, ResultExt}};

pub async fn run_tool(
	tool_call: &genai::chat::ToolCall,
	check_command: Option<&str>,
	shell_tools: &HashMap<String, ShellTool>,
) -> serde_json::Value {
	let raw_args = tool_call.fn_arguments.clone();

	use async_result_ext::AsyncResultExt;
	match tool_call.fn_name.as_str() {
		"grep" => {
			serde_json::from_value::<GrepArgs>(raw_args).map_err(utils::json_err)
				.async_map(async |a| run_grep(a).await.map_err(utils::json_err).into_ok_or_err())
				.await.into_ok_or_err()
		},
		"read_file" => {
			serde_json::from_value::<ReadFileArgs>(raw_args).map_err(utils::json_err)
				.async_map(async |a| run_read_file(a).await.map_err(utils::json_err).into_ok_or_err())
				.await.into_ok_or_err()
		},
		"create_file" => {
			serde_json::from_value::<CreateFileArgs>(raw_args).map_err(utils::json_err)
				.async_map(async |a| run_create_file(a).await.map_err(utils::json_err).into_ok_or_err())
				.await.into_ok_or_err()
		},
		"write_chunk" => {
			serde_json::from_value::<WriteChunkArgs>(raw_args).map_err(utils::json_err)
				.async_map(async |a| run_write_chunk(a).await.map_err(utils::json_err).into_ok_or_err())
				.await.into_ok_or_err()
		},
		"check_command" if check_command.is_some() => {
			run_check_command(check_command.unwrap())
				.await.map_err(utils::json_err).into_ok_or_err()
		},
		shell_tool if shell_tools.contains_key(shell_tool) => {
			let shell_tool = shell_tools.get(shell_tool).unwrap();
			serde_json::from_value::<ShellCommandArgs>(raw_args).map_err(utils::json_err)
				.async_map(async |a| run_general_shell(shell_tool, a).await.map_err(utils::json_err).into_ok_or_err())
				.await.into_ok_or_err()
		},
		unknown_tool => serde_json::json!({ "error": format!("unknown tool: {unknown_tool}") }),
	}
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

	let tool = genai::chat::Tool::new(&shell_prefix)
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

	Ok(((shell_prefix, shell_tool), tool))
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
	else { Ok(serde_json::json!("check command ran successfully")) }
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
	else { Ok(serde_json::json!("check command ran successfully")) }
}


// #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
// #[schemars(rename = "tree arguments")]
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
#[schemars(rename = "grep arguments")]
struct GrepArgs {
	#[serde(default = "utils::default_dot")]
	/// the dir to grep through, defaults to `.`
	dir: std::path::PathBuf,
	/// the pattern to grep for
	pattern: String,
}
// TODO should just allow a pipe through to raw grep itself, with some sanitization to ensure the allow/disallow rules are followed
pub fn tool_grep() -> genai::chat::Tool {
	let schema = schemars::schema_for!(GrepArgs).to_value();
	genai::chat::Tool::new("grep")
		.with_description("grep recursively through `dir` to find `pattern`, using ripgrep")
		.with_schema(schema)
}
async fn run_grep(GrepArgs { dir, pattern }: GrepArgs) -> JsonResult {
	utils::dir_is_allowed_based_on_gitignore(&dir)?;

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
#[schemars(rename = "read_file arguments")]
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
	let schema = schemars::schema_for!(ReadFileArgs).to_value();

	genai::chat::Tool::new("read_file")
		.with_description("read the contents of `file`")
		// maybe using `starting_from` or `ending_at` to limit how much is read
		.with_schema(schema)
}
async fn run_read_file(ReadFileArgs { file }: ReadFileArgs) -> JsonResult {
	utils::path_is_allowed_based_on_gitignore(&file)?;

	let contents = tokio::fs::read_to_string(&file)
		.await.map_err(|e| format!("unable to read file {}: {e}", file.display()))?;

	Ok(serde_json::json!(contents))
}


#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(rename = "create_file arguments")]
/// create a new `file` with `contents`
struct CreateFileArgs {
	/// the path of the file to create. must not already exist
	file: std::path::PathBuf,
	/// the contents of the new file
	contents: String,
}
pub fn tool_create_file() -> genai::chat::Tool {
	let schema = schemars::schema_for!(CreateFileArgs).to_value();
	genai::chat::Tool::new("create_file")
		.with_schema(schema)
}
async fn run_create_file(CreateFileArgs { file, contents }: CreateFileArgs) -> JsonResult {
	if let Some(parent) = file.parent() {
		tokio::fs::create_dir_all(parent)
			.await.map_err(|e| format!("unable to create parent directories for {}: {e}", file.display()))?;
	}

	use tokio::io::AsyncWriteExt;
	tokio::fs::OpenOptions::new().write(true).create_new(true)
		.open(&file).await.map_err(|e| format!("unable to open file {}: {e}", file.display()))?
		.write_all(contents.as_bytes()).await.map_err(|e| format!("unable to write to {}: {e}", file.display()))?;

	Ok(serde_json::json!(format!("successfully created {}", file.display())))
}


#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(rename = "write_chunk arguments")]
struct WriteChunkArgs {
	/// the file in which to write the chunk of text
	file: std::path::PathBuf,
	/// the text to be replaced by this write. must match uniquely including whitespace
	replace_text: String,
	/// the final text that should end up in the file instead of replace_text, including correct whitespace
	with_text: String,
}
pub fn tool_write_chunk() -> genai::chat::Tool {
	let schema = schemars::schema_for!(WriteChunkArgs).to_value();
	genai::chat::Tool::new("write_chunk")
		.with_description("write a chunk of text into `file`, replacing `replace_text` with `with_text`")
		.with_schema(schema)
}
async fn run_write_chunk(WriteChunkArgs { file, replace_text, with_text }: WriteChunkArgs) -> JsonResult {
	utils::path_is_allowed_based_on_gitignore(&file)?;

	if replace_text.is_empty() {
		return Err(format!("replace_text must not be empty"));
	}

	let content = tokio::fs::read_to_string(&file)
		.await.map_err(|e| format!("io error while reading {}: {e}", file.display()))?;

	match content.matches(&replace_text).count() {
		0 => Err(format!("replace_text {replace_text} not found in {}", file.display())),
		1 => {
			let new_content = content.replacen(&replace_text, &with_text, 1);
			tokio::fs::write(&file, new_content)
				.await.map_err(|e| format!("io error while reading {}: {e}", file.display()))?;

			Ok(serde_json::json!(format!("successful replacement in {} of: {replace_text}", file.display())))
		},
		n => Err(format!("replace_text {replace_text} found {n} times, expected exactly 1")),
	}
}
