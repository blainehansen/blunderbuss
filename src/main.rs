use serde_json::json;

type JsonResult = Result<serde_json::Value, String>;
// TODO make this configurable and more reasonable
const MAX_STEPS: usize = 20;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	// tracing_subscriber::fmt()
	// 	.with_env_filter(EnvFilter::new("genai=debug"))
	// 	// .with_max_level(tracing::Level::DEBUG) // To enable all sub-library tracing
	// 	.init();
	let question = tokio::fs::read_to_string("klungo.local.md").await?;

	// let re = regex::Regex::new(r"/check\[([^\n\]]+)\]").unwrap();
	// if let Some(caps) = re.captures(&question) {
	// 	let full_match = caps.get(0).map(|m| m.as_str());
	// 	let check = caps.get(1).map(|m| m.as_str());

	// 	println!("Full match: {:?}", full_match);
	// 	println!("Check: {:?}", check);
	// }


	let client = genai::Client::builder().with_model_mapper_fn(|_model_iden| {
		// Ok(genai::ModelIden::from_static(genai::adapter::AdapterKind::Gemini, "gemma-4-31b-it"))
		Ok(genai::ModelIden::from_static(genai::adapter::AdapterKind::Gemini, "gemma-4-26b-a4b-it"))
	}).build()?;

	let tools = vec![
		// tool_tree(),
		tool_grep(),
		tool_read_file(),
		tool_create_file(),
		tool_write_chunk(),
	];

	let mut chat_req = genai::chat::ChatRequest::default()
		.with_system(include_str!("./system.md"))
		.with_tools(tools);

	chat_req = chat_req.append_message(genai::chat::ChatMessage::user(question));

	// TODO at the beginning we push the basic "temp" git commit that's formatted in such a way that we can easily undo it
	// https://itnext.io/multitask-like-a-pro-with-the-wip-commit-2f4d40ca0192
	// alias gwip='git add -A; git rm $(git ls-files --deleted) 2> /dev/null; git commit --no-verify --no-gpg-sign --message "--wip-- [skip ci]"'

	// alias gunwip='git rev-list --max-count=1 --format="%s" HEAD | grep -q "\--wip--" && git reset HEAD~1'

	// # Similar to `gunwip` but recursive "Unwips" all recent `--wip--` commits not just the last one
	// function gunwipall() {
	//   local _commit=$(git log --grep='--wip--' --invert-grep --max-count=1 --format=format:%H)

	//   # Check if a commit without "--wip--" was found and it's not the same as HEAD
	//   if [[ "$_commit" != "$(git rev-parse HEAD)" ]]; then
	//     git reset $_commit || return 1
	//   fi
	// }

	// how to actually do this? we have two main loops
	// one for the "are we happy" question, basically whether the configured or passed check command has passed
	// and then the actual max steps
	// probably has to just be a single while loop that increments the step and keeps track

	// let check_command = "cargo check";

	let mut step = 0;
	loop {
		if step >= MAX_STEPS {
			println!("klungo failed to achieved its goal within the max steps of {MAX_STEPS}");
			// TODO
			break
		}

		// actually execute this step of the chat
		use backon::Retryable;
		let chat_res = (|| async { client.exec_chat("", chat_req.clone(), None).await })
			.retry(backon::ExponentialBuilder::default())
			.when(genai_error_is_retryable)
			.notify(|err, dur| {
				println!("retrying error {:?} after {:?}", err, dur);
			})
			.await?;

		// process all the tool calls in parallel
		let tool_responses = futures::future::join_all(
			chat_res.tool_calls().into_iter().map(async |tool_call| {
				let output = run_tool(&tool_call).await;
				genai::chat::ToolResponse::from_tool_call(&tool_call, output.to_string())
			})
		).await;

		// append this round to the history (after running tools for ownership reasons)
		chat_req = chat_req.append_message(
			genai::chat::ChatMessage::assistant(chat_res.content)
				.with_reasoning_content(chat_res.reasoning_content)
		);

		// there aren't any more calls (and therefore any more responses), the llm says its done
		if tool_responses.is_empty() {
			// TODO check if it's *actually* done with the configured

			break;
		}
		// otherwise append the tool responses
		chat_req = chat_req.append_message(tool_responses);

		step += 1;
	}

	// chat_req.messages.into_iter()
	let chat_json = serde_json::to_vec_pretty(&chat_req.messages)?;
	tokio::fs::write("output.local.json", chat_json).await?;

	Ok(())
}

async fn run_tool(tool_call: &genai::chat::ToolCall) -> serde_json::Value {
	let raw_args = tool_call.fn_arguments.clone();

	use async_result_ext::AsyncResultExt;
	match tool_call.fn_name.as_str() {
		"grep" => {
			serde_json::from_value::<GrepArgs>(raw_args).map_err(json_err)
				.async_map(async |a| run_grep(a).await.map_err(json_err).into_ok_or_err())
				.await.into_ok_or_err()
		}
		"read_file" => {
			serde_json::from_value::<ReadFileArgs>(raw_args).map_err(json_err)
				.async_map(async |a| run_read_file(a).await.map_err(json_err).into_ok_or_err())
				.await.into_ok_or_err()
		}
		"create_file" => {
			serde_json::from_value::<CreateFileArgs>(raw_args).map_err(json_err)
				.async_map(async |a| run_create_file(a).await.map_err(json_err).into_ok_or_err())
				.await.into_ok_or_err()
		}
		"write_chunk" => {
			serde_json::from_value::<WriteChunkArgs>(raw_args).map_err(json_err)
				.async_map(async |a| run_write_chunk(a).await.map_err(json_err).into_ok_or_err())
				.await.into_ok_or_err()
		}
		other => json!({ "error": format!("unknown tool: {other}") }),
	}
}

// use inquire::{error::InquireError, Select};

// fn main() {
// 	let options: Vec<&str> = vec![
// 		"Banana", "Apple", "Strawberry", "Grapes",
// 		"Lemon", "Tangerine", "Watermelon", "Orange", "Pear", "Avocado", "Pineapple",
// 	];

// 	let ans: Result<&str, InquireError> = Select::new("What's your favorite fruit?", options)
// 		.with_page_size(20)
// 		.without_help_message()
// 		.prompt();

// 	match ans {
// 		Ok(choice) => println!("{}! That's mine too!", choice),
// 		Err(_) => println!("There was an error, please try again"),
// 	}
// }


// #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
// #[schemars(rename = "tree arguments")]
// struct Tree {
// 	#[serde(default = "default_dot")]
// 	dir: String,
// }
// fn tool_tree() -> genai::chat::Tool {
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
	#[serde(default = "default_dot")]
	/// the dir to grep through, defaults to `.`
	dir: std::path::PathBuf,
	/// the pattern to grep for
	pattern: String,
}
fn tool_grep() -> genai::chat::Tool {
	let schema = schemars::schema_for!(GrepArgs).to_value();
	genai::chat::Tool::new("grep")
		.with_description("grep recursively through `dir` to find `pattern`, using ripgrep")
		.with_schema(schema)
}
async fn run_grep(GrepArgs { dir, pattern }: GrepArgs) -> JsonResult {
	dir_is_allowed_based_on_gitignore(&dir)?;

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

	return Ok(json!({ "matches": matches }))
}
fn parse_grep_line(line: &str) -> Option<serde_json::Value> {
	let mut parts = line.splitn(3, ':');
	let path = parts.next()?;
	let line = parts.next()?.parse::<usize>().ok()?;
	let text = parts.next().unwrap_or("").trim();

	Some(json!({ "path": path, "line": line, "text": text }))
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
fn tool_read_file() -> genai::chat::Tool {
	let schema = schemars::schema_for!(ReadFileArgs).to_value();

	genai::chat::Tool::new("read_file")
		.with_description("read the contents of `file`")
		// maybe using `starting_from` or `ending_at` to limit how much is read
		.with_schema(schema)
}
async fn run_read_file(ReadFileArgs { file }: ReadFileArgs) -> JsonResult {
	path_is_allowed_based_on_gitignore(&file)?;

	let contents = tokio::fs::read_to_string(&file)
		.await.map_err(|e| format!("unable to read file {}: {e}", file.display()))?;

	Ok(json!(contents))
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
fn tool_create_file() -> genai::chat::Tool {
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

	Ok(json!(format!("successfully created {}", file.display())))
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
fn tool_write_chunk() -> genai::chat::Tool {
	let schema = schemars::schema_for!(WriteChunkArgs).to_value();
	genai::chat::Tool::new("write_chunk")
		.with_description("write a chunk of text into `file`, replacing `replace_text` with `with_text`")
		.with_schema(schema)
}
async fn run_write_chunk(WriteChunkArgs { file, replace_text, with_text }: WriteChunkArgs) -> JsonResult {
	path_is_allowed_based_on_gitignore(&file)?;

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

			Ok(json!(format!("successful replacement in {} of: {replace_text}", file.display())))
		},
		n => Err(format!("replace_text {replace_text} found {n} times, expected exactly 1")),
	}
}



fn genai_error_is_retryable(e: &genai::Error) -> bool {
	// e.headers()

	if let Some(status) = e.status() {
		return status.is_server_error()
			|| status == http::StatusCode::TOO_MANY_REQUESTS
			|| status == http::StatusCode::REQUEST_TIMEOUT
	}
	return false
}

trait ResultExt<T> {
	fn into_ok_or_err(self) -> T;
}

impl<T> ResultExt<T> for Result<T, T> {
	fn into_ok_or_err(self) -> T {
		let (Ok(i) | Err(i)) = self; i
	}
}

fn json_err(message: impl ToString) -> serde_json::Value {
	json!({ "error": message.to_string() })
}

fn default_dot() -> std::path::PathBuf {
	std::path::PathBuf::from(".")
}

fn path_is_allowed_based_on_gitignore(path: &std::path::Path) -> Result<(), String> {
	let (ig, _) = ignore::gitignore::Gitignore::new(".");

	ig.matched(path, false).is_ignore()
		.ok_or_else(|| format!("file {} is ignored, meaning it is off limits", path.display()))
}

fn dir_is_allowed_based_on_gitignore(path: &std::path::Path) -> Result<(), String> {
	let (ig, _) = ignore::gitignore::Gitignore::new(".");

	ig.matched(path, true).is_ignore()
		.ok_or_else(|| format!("dir {} is ignored, meaning it is off limits", path.display()))
}
