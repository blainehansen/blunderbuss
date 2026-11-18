use std::collections::HashMap;
use anyhow::Context;

mod tools;
mod utils;
mod parser;

type JsonResult = Result<serde_json::Value, String>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	// tracing_subscriber::fmt()
	// 	.with_env_filter(EnvFilter::new("genai=debug"))
	// 	// .with_max_level(tracing::Level::DEBUG)
	// 	.init();

	tokio::select! {
		result = tokio::signal::ctrl_c() => {
			println!("ctrl+c received, popping the temp git checkpoint commit if there is one");
			let _ = hard_reset_checkpoint_commit().await;
			result?
		},
		result = chat_loop() => {
			match result {
				Ok(()) => {
					// nothing to do in this case, everything succeeded, let's go home
				},
				Err(err) => {
					println!("some error encountered, popping the temp git checkpoint commit if there is one");
					let _ = hard_reset_checkpoint_commit().await;
					return Err(err);
				},
			}
		},
	}

	Ok(())
}

#[derive(Debug)]
struct ChatParams {
	model: String,
	maxturns: usize,
	shell_tools: HashMap<String, tools::ShellTool>,
	check_command: Option<String>,
	path_env: utils::PathEnv,
}

async fn create_new_chat(log_writer: &LogWriter) -> anyhow::Result<(ChatParams, genai::chat::ChatRequest)> {
	let UserConfig { default_model, default_maxturns, tools: default_tools } = try_klungo_config().await?;
	let instructions = tokio::fs::read_to_string("klungo.local.md").await
		// TODO allow piping stdin at some point
		.map_err(|e| anyhow::anyhow!("couldn't read the klungo.local.md file, which is the whole way klungo works!: {e}"))?;

	let (
		parser::RawParams {
			tools: instruction_tools, model, maxturns, allow: _, disallow: _, check: check_command,
		},
		text,
	) = parser::parse_instructions(&instructions)
		.map_err(|e| anyhow::anyhow!(e))?;

	let model = model
		.or(default_model)
		.ok_or_else(|| anyhow::anyhow!("you don't have a model specified! you have to either provide one using the `model` frontmatter param in klungo.local.md or `default_model` in ~/.klungo.config.yml"))?;

	const DEFAULT_MAXTURNS: usize = 20;
	let maxturns = maxturns
		.or(default_maxturns)
		.unwrap_or(DEFAULT_MAXTURNS);

	// TODO add allow and disallow
	let path_env = utils::PathEnv::new()?;

	let (shell_tools, chat_shell_tools) = default_tools.unwrap_or_default().into_iter().chain(instruction_tools.unwrap_or_default().into_iter())
		.map(|m| tools::tool_general_shell(m))
		.collect::<Result<(HashMap<_, _>, Vec<_>), _>>()
		.map_err(|e| anyhow::anyhow!(e))?;

	let mut all_tools = [
		// tool_tree(),
		tools::tool_grep(),
		tools::tool_read_file(),
		tools::tool_write_file(),
		tools::tool_replace_span(),
		tools::tool_replace_section(),
	].into_iter()
		.chain(chat_shell_tools)
		.collect::<Vec<_>>();

	if let Some(check_command) = &check_command {
		all_tools.push(tools::tool_check_command(check_command));
	}

	let mut chat_req = genai::chat::ChatRequest::default()
		.with_system(include_str!("./system.md"))
		.with_tools(all_tools);

	println!("tools: {}", serde_json::to_string_pretty(chat_req.tools.as_deref().unwrap_or_default()).unwrap());

	let msg = genai::chat::ChatMessage::user(text);
	log_writer.log_chat_message(msg.clone());
	chat_req = chat_req.append_message(msg);

	println!("using model {model}");
	println!("using maxturns {maxturns}");
	for shell_tool in shell_tools.keys() {
		println!("using shell tool: {shell_tool}");
	}
	if let Some(check_command) = &check_command {
		println!("using check command {check_command}");
	}

	let instructions = ChatParams { model, maxturns, shell_tools, check_command, path_env };
	Ok((instructions, chat_req))
}

#[derive(Debug, Default, serde::Deserialize)]
struct UserConfig {
	default_model: Option<String>,
	default_maxturns: Option<usize>,
	tools: Option<Vec<String>>,
}

async fn try_klungo_config() -> anyhow::Result<UserConfig> {
	let klungo_config_path = std::env::home_dir().unwrap_or("~/".into()).join(".klungo.config.yml");
	if !tokio::fs::try_exists(&klungo_config_path).await.unwrap_or(false) {
		return Ok(UserConfig::default())
	}

	let file_contents = tokio::fs::read_to_string(klungo_config_path).await
		.context("wasn't able to read .klungo.config.yml even though it exists?")?;

	yaml_serde::from_str(&file_contents)
		.context("wasn't able to parse .klungo.config.yml")
}

async fn chat_loop() -> anyhow::Result<()> {
	let log_writer = spawn_file_appender().await?;

	let (mut p, mut chat_req) = create_new_chat(&log_writer).await?;

	// we just use the out of the box genai behavior, and all its model/auth resolver magic
	let client = genai::Client::new()?;
	let mut turn = 0;
	let mut ran_out_of_turns = false;
	push_checkpoint_commit().await?;
	loop {
		if turn >= p.maxturns {
			ran_out_of_turns = true;
			break
		}

		// execute this turn
		use backon::Retryable;
		let chat_res = (|| async { client.exec_chat(&p.model, chat_req.clone(), None).await })
			.retry(backon::ExponentialBuilder::default())
			.when(genai_error_is_retryable)
			.notify(|err, dur| {
				println!("retrying error {:?} after {:?}", err, dur);
			})
			.await?;

		let genai::chat::ChatResponse { content, reasoning_content, .. } = chat_res;

		let tool_calls = content.tool_calls();
		let mut tool_responses = Vec::with_capacity(tool_calls.len());
		// we execute the tool calls sequentially for now to avoid possible headaches with them interfering
		// something fancy in the future would be to divide up the tools by the things they could effect (files, whatever)
		// however that's easier said than done, since the tools the user specifies could mess things up
		for tool_call in tool_calls.into_iter() {
			let output = tools::run_tool(&tool_call, p.check_command.as_deref(), &p.shell_tools, &p.path_env).await;

			let tool_response = genai::chat::ToolResponse::from_tool_call(&tool_call, output.to_string());
			tool_responses.push(tool_response);
		}
		// append this round to the history
		let msg = genai::chat::ChatMessage::assistant(content).with_reasoning_content(reasoning_content);
		log_writer.log_chat_message(msg.clone());
		chat_req = chat_req.append_message(msg);

		// the llm is still doing things, so keep at it
		if !tool_responses.is_empty() {
			let msg: genai::chat::ChatMessage = tool_responses.into();
			log_writer.log_chat_message(msg.clone());
			chat_req = chat_req.append_message(msg);
			turn += 1;
			continue;
		}

		// there aren't any more calls (and therefore any more responses), the llm says its done
		if let Some(check_command) = &p.check_command {
			println!("klungo says it's finished, running your check command {check_command}");
			let output = tokio::process::Command::new("sh").arg("-c").arg(check_command)
				.output().await.with_context(|| format!("wasn't able to call sh -c '{check_command}'"))?;

			if !output.status.success() {
				let stdout = String::from_utf8_lossy(&output.stdout);
				let stderr = String::from_utf8_lossy(&output.stderr);
				println!("check command {check_command} failed! sending the output back to klungo to try again");

				let check_command_feedback = format!("the check command {check_command} failed.\nstdout:\n{stdout}\n\n\nstderr:\n{stderr}");
				let msg = genai::chat::ChatMessage::user(check_command_feedback);
				log_writer.log_chat_message(msg.clone());
				chat_req = chat_req.append_message(msg);
				turn += 1;
				continue;
			}
			println!("check command {check_command} succeeded!");
		}

		const LOOKS_GOOD: &'static str = "looks good";
		const NOPE_REVERT: &'static str = "nope revert";
		const TRY_AGAIN_ANEW: &'static str = "try again (start new chat from klungo.local.md)";
		const TRY_AGAIN_FROM_HERE: &'static str = "try again (continue this chat)";
		let options: Vec<&str> = vec![LOOKS_GOOD, NOPE_REVERT, TRY_AGAIN_ANEW, TRY_AGAIN_FROM_HERE];
		let page_size = options.len();

		let choice = inquire::Select::new("How's that?", options)
			.with_page_size(page_size)
			.without_help_message()
			.prompt().context("there was some issue with the inquire library?")?;

		match choice {
			LOOKS_GOOD => {
				// pop the temp git commit
				soft_reset_checkpoint_commit().await?;
				break;
			},
			NOPE_REVERT => {
				// revert and hard reset
				hard_reset_checkpoint_commit().await?;
				// stop trying, the user doesn't want to bother
				break;
			},
			TRY_AGAIN_ANEW => {
				// this tries again and rereads klungo.local.md
				hard_reset_checkpoint_commit().await?;
				(p, chat_req) = create_new_chat(&log_writer).await?;
				turn = 0;
				continue;
			},
			TRY_AGAIN_FROM_HERE => {
				// this tries again with the context we already have
				hard_reset_checkpoint_commit().await?;
				let msg = genai::chat::ChatMessage::user(
					"I'm unsatisfied with that attempt. everything has been reverted, try again",
				);
				log_writer.log_chat_message(msg.clone());
				chat_req = chat_req.append_message(msg);
				turn = 0;
				continue;
			},
			unknown => { return Err(anyhow::anyhow!("how did you choose this? {unknown}")) }
		}
	}

	log_writer.finish().await;

	if ran_out_of_turns {
		return Err(anyhow::anyhow!("klungo failed to finish within the max turns: {}", p.maxturns))
	}

	Ok(())
}

#[derive(Debug)]
struct LogWriter {
	sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
	handle: tokio::task::JoinHandle<()>,
}

impl LogWriter {
	fn log_chat_message(&self, msg: genai::chat::ChatMessage) {
		let msg = match serde_json::to_vec_pretty(&msg) {
			Err(e) => { eprintln!("failed to serialize: {}", e); return; },
			Ok(msg) => { msg },
		};

		if let Err(e) = self.sender.send(msg) {
			eprintln!("failed to log chat message: {:?}", e.0);
		}
	}

	// fn log_whatever(&self, msg: impl Into<Vec<u8>>)  {
	// 	if let Err(e) = self.sender.send(msg.into()) {
	// 		eprintln!("failed to log chat message: {}", String::from_utf8_lossy(&e.0));
	// 	}
	// }

	async fn finish(self) {
		let LogWriter { sender, handle } = self;
		drop(sender);
		let _ = handle.await;
	}
}

async fn spawn_file_appender() -> anyhow::Result<LogWriter> {
	let path = "klungo.output.local.json";
	let mut file = tokio::fs::OpenOptions::new().create(true).truncate(true).write(true).open(path).await?;
	let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

	let handle = tokio::spawn(async move {
		use tokio::io::AsyncWriteExt;
		while let Some(mut msg) = receiver.recv().await {
			msg.push(b'\n');

			let _ = file.write_all(&msg).await
				.inspect_err(|e| eprintln!("failed to write to klungo.output.local.json: {e}"));
			let _ = file.flush().await
				.inspect_err(|e| eprintln!("failed to flush klungo.output.local.json: {e}"));
		}
	});

	Ok(LogWriter { sender, handle })
}

// https://itnext.io/multitask-like-a-pro-with-the-wip-commit-2f4d40ca0192
async fn push_checkpoint_commit() -> anyhow::Result<()> {
	let output = tokio::process::Command::new("sh").arg("-c")
		.arg("git add -A; git rm $(git ls-files --deleted) 2> /dev/null; git status --porcelain || git commit --no-verify --no-gpg-sign -m '--klungo-checkpoint--'")
		.output().await?;

	utils::demand_command_success(output, "failed to push the temporary git checkpoint commit")
}
async fn soft_reset_checkpoint_commit() -> anyhow::Result<()> {
	let output = tokio::process::Command::new("sh").arg("-c")
		.arg(r#"git rev-list --max-count=1 --format="%s" HEAD | grep -q "\--klungo-checkpoint--" && git reset HEAD~1"#)
		.output().await?;

	utils::demand_command_success(output, "failed to pop the temporary git checkpoint commit")
}
async fn hard_reset_checkpoint_commit() -> anyhow::Result<()> {
	let output = tokio::process::Command::new("sh").arg("-c")
		.arg(r#"git rev-list --max-count=1 --format="%s" HEAD | grep -q "\--klungo-checkpoint--" && git reset --hard HEAD~1 && git clean -fd"#)
		.output().await?;

	utils::demand_command_success(output, "failed to pop the temporary git checkpoint commit")
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
