use std::collections::HashMap;
use anyhow::Context;
use utils::OutputExt;

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
			let _ = hard_reset_and_pop_checkpoint_commit().await;
			result?
		},
		result = chat_loop() => {
			match result {
				Ok(()) => {
					// nothing to do in this case, everything succeeded, let's go home
				},
				Err(err) => {
					println!("some error encountered, popping the temp git checkpoint commit if there is one");
					let _ = hard_reset_and_pop_checkpoint_commit().await;
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
	ready_command: Option<String>,
	path_env: utils::PathEnv,
}

async fn create_new_chat(
	log_writer: &LogWriter,
) -> anyhow::Result<(ChatParams, genai::chat::ChatRequest, genai::Client)> {
	let UserConfig { default_model, default_maxturns, tools: default_tools, api_keys } = try_blunderbuss_config().await?;
	let instructions = tokio::fs::read_to_string("blunderbuss.local.md").await
		// TODO allow piping stdin at some point
		.map_err(|e| anyhow::anyhow!("couldn't read the blunderbuss.local.md file, which is the whole way blunderbuss works!: {e}"))?;

	let (
		parser::RawParams {
			tools: instruction_tools, model, maxturns, allow: _, disallow: _, check: check_command, ready: ready_command,
		},
		text,
	) = parser::parse_instructions(&instructions)
		.map_err(|e| anyhow::anyhow!(e))?;

	let model = model
		.or(default_model)
		.ok_or_else(|| anyhow::anyhow!("you don't have a model specified! you have to either provide one using the `model` frontmatter param in blunderbuss.local.md or `default_model` in ~/.blunderbuss.config.yml"))?;

	const DEFAULT_MAXTURNS: usize = 100;
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

	log_writer.log_serde(chat_req.tools.as_deref().unwrap_or_default());

	let msg = genai::chat::ChatMessage::user(text);
	log_writer.log_serde(&msg);
	chat_req = chat_req.append_message(msg);

	println!("using model {model}");
	println!("using maxturns {maxturns}");
	for shell_tool in shell_tools.keys() {
		println!("using shell tool: {shell_tool}");
	}
	if let Some(check_command) = &check_command {
		println!("using check command {check_command}");
	}
	if let Some(ready_command) = &ready_command {
		println!("using ready command {ready_command}");
	}

	let auth_resolver = genai::resolver::AuthResolver::from_resolver_fn(move |model_iden: genai::ModelIden| {
		let genai::ModelIden { adapter_kind, .. } = model_iden;

		let api_key = api_keys.zip(adapter_kind.default_key_env_name())
			.and_then(|(api_keys, k)| api_keys.get(k).map(String::to_owned));

		match api_key {
			Some(key) => Ok(Some(genai::resolver::AuthData::from_single(key))),
			// fall back to genai's default
			None => Ok(None),
		}
	});
	let client = genai::Client::builder()
		.with_auth_resolver(auth_resolver)
		.build()?;

	let instructions = ChatParams { model, maxturns, shell_tools, check_command, path_env, ready_command };
	Ok((instructions, chat_req, client))
}

#[derive(Debug, Default, serde::Deserialize)]
struct UserConfig {
	default_model: Option<String>,
	default_maxturns: Option<usize>,
	tools: Option<Vec<String>>,
	api_keys: Option<HashMap<String, String>>,
}

async fn try_blunderbuss_config() -> anyhow::Result<UserConfig> {
	let blunderbuss_config_path = std::env::home_dir().unwrap_or("~/".into()).join(".blunderbuss.config.yml");
	if !tokio::fs::try_exists(&blunderbuss_config_path).await.unwrap_or(false) {
		return Ok(UserConfig::default())
	}

	let file_contents = tokio::fs::read_to_string(blunderbuss_config_path).await
		.context("wasn't able to read .blunderbuss.config.yml even though it exists?")?;

	yaml_serde::from_str(&file_contents)
		.context("wasn't able to parse .blunderbuss.config.yml")
}

async fn chat_loop() -> anyhow::Result<()> {
	let log_writer = LogWriter::spawn_new().await?;

	let (mut p, mut chat_req, mut client) = create_new_chat(&log_writer).await?;

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
			.retry(
				backon::ExponentialBuilder::default()
					.with_min_delay(std::time::Duration::from_secs(20))
					.with_max_delay(std::time::Duration::from_secs(120))
					.with_max_times(10)
			)
			.when(genai_error_is_retryable)
			.notify(|err, dur| {
				log_writer.log_msg(format!("error, retrying after {dur:?}: {:?}", err).into_bytes());
				println!("error with status {:?}, retrying after {dur:?}", err.status());
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
		log_writer.log_serde(&msg);
		chat_req = chat_req.append_message(msg);

		// the llm is still doing things, so keep at it
		if !tool_responses.is_empty() {
			let msg: genai::chat::ChatMessage = tool_responses.into();
			log_writer.log_serde(&msg);
			chat_req = chat_req.append_message(msg);
			turn += 1;
			continue;
		}

		// there aren't any more calls (and therefore any more responses), the llm says its done
		if let Some(check_command) = &p.check_command {
			println!("blunderbuss says it's finished, running your check command {check_command}");
			let output = tokio::process::Command::new("sh").arg("-c").arg(check_command)
				.output().await.with_context(|| format!("wasn't able to call sh -c '{check_command}'"))?;

			if !output.status.success() {
				let stdout = String::from_utf8_lossy(&output.stdout);
				let stderr = String::from_utf8_lossy(&output.stderr);
				println!("check command {check_command} failed! sending the output back to blunderbuss to try again");

				let check_command_feedback = format!("the check command {check_command} failed.\nstdout:\n{stdout}\n\n\nstderr:\n{stderr}");
				let msg = genai::chat::ChatMessage::user(check_command_feedback);
				log_writer.log_serde(&msg);
				chat_req = chat_req.append_message(msg);
				turn += 1;
				continue;
			}
			println!("check command {check_command} succeeded!");
		}

		if let Some(ready_command) = &p.ready_command {
			println!("running your ready command {ready_command}");
			tokio::process::Command::new("sh").arg("-c").arg(ready_command)
				.output().await.with_context(|| format!("wasn't able to call sh -c '{ready_command}'"))?;
		}

		const LOOKS_GOOD: &'static str = "looks good (will remove the blunderbuss checkpoint commit and mix blunderbuss's changes in with yours)";
		const NOPE_REVERT: &'static str = "nope revert (will *hard* git reset and remove the blunderbuss checkpoint commit)";
		const TRY_AGAIN_ANEW: &'static str = "try again (will *hard* git reset and start a new chat from blunderbuss.local.md)";
		// const TRY_AGAIN_FROM_HERE: &'static str = "try again (continue from this existing context)";
		// let options: Vec<&str> = vec![LOOKS_GOOD, NOPE_REVERT, TRY_AGAIN_ANEW, TRY_AGAIN_FROM_HERE];
		let options: Vec<&str> = vec![LOOKS_GOOD, NOPE_REVERT, TRY_AGAIN_ANEW];
		let page_size = options.len();

		let choice = inquire::Select::new("How's that?", options)
			.with_page_size(page_size)
			.without_help_message()
			.prompt().context("there was some issue with the inquire library?")?;

		match choice {
			LOOKS_GOOD => {
				// pop the temp git commit
				pop_checkpoint_commit().await?;
				break;
			},
			NOPE_REVERT => {
				// revert and hard reset
				hard_reset_and_pop_checkpoint_commit().await?;
				// stop trying, the user doesn't want to bother
				break;
			},
			TRY_AGAIN_ANEW => {
				// this tries again and rereads blunderbuss.local.md
				hard_reset_and_pop_checkpoint_commit().await?;
				(p, chat_req, client) = create_new_chat(&log_writer).await?;
				turn = 0;
				continue;
			},
			// TRY_AGAIN_FROM_HERE => {
			// 	// this tries again with the context we already have
			// 	hard_reset_and_pop_checkpoint_commit().await?;
			// 	let msg = genai::chat::ChatMessage::user(
			// 		"I'm unsatisfied with that attempt. everything has been reverted, try again",
			// 	);
			// 	log_writer.log_serde(&msg);
			// 	chat_req = chat_req.append_message(msg);
			// 	turn = 0;
			// 	continue;
			// },
			unknown => { return Err(anyhow::anyhow!("how did you choose this? {unknown}")) }
		}
	}

	log_writer.finish().await;

	if ran_out_of_turns {
		return Err(anyhow::anyhow!("blunderbuss failed to finish within the max turns: {}", p.maxturns))
	}

	Ok(())
}

#[derive(Debug)]
struct LogWriter {
	sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
	handle: tokio::task::JoinHandle<()>,
}

impl LogWriter {
	async fn spawn_new() -> anyhow::Result<LogWriter> {
		let path = "blunderbuss.output.local.yml";
		let mut file = tokio::fs::OpenOptions::new().create(true).truncate(true).write(true).open(path).await?;
		let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

		let handle = tokio::spawn(async move {
			use tokio::io::AsyncWriteExt;
			while let Some(mut msg) = receiver.recv().await {
				msg.extend_from_slice(b"\n---\n\n");

				let _ = file.write_all(&msg).await
					.inspect_err(|e| eprintln!("failed to write to blunderbuss.output.local.yml: {e}"));
				let _ = file.flush().await
					.inspect_err(|e| eprintln!("failed to flush blunderbuss.output.local.yml: {e}"));
			}
		});

		Ok(LogWriter { sender, handle })
	}

	fn log_msg(&self, msg: Vec<u8>) {
		if let Err(e) = self.sender.send(msg) {
			eprintln!("failed to log chat message: {}", String::from_utf8_lossy(&e.0));
		}
	}

	fn log_serde(&self, msg: impl serde::Serialize) {
		let msg = match yaml_serde::to_string(&msg) {
			Err(e) => { eprintln!("failed to serialize: {}", e); return; },
			Ok(msg) => { msg.into_bytes() },
		};

		self.log_msg(msg);
	}

	async fn finish(self) {
		let LogWriter { sender, handle } = self;
		drop(sender);
		let _ = handle.await;
	}
}

/// push the blunderbuss checkpoint commit if there are existing uncommitted changes
/// this is to allow the user to cleanly separate blunderbuss's changes from their existing ones
async fn push_checkpoint_commit() -> anyhow::Result<()> {
	// the if statement short circuits with a successful status code if there are no changes
	// if the git commit sequence happens the status code of that is passed through
	tokio::process::Command::new("sh").arg("-c")
		.arg(r#"if [ -n "$(git status --porcelain)" ]; then git add -A; git commit --no-verify --no-gpg-sign -m '--blunderbuss-checkpoint--'; fi"#)
		.output().await?.demand_status_success("failed to push the temporary git checkpoint commit")?;

	Ok(())
}
/// get rid of the blunderbuss checkpoint commit so that the changes made by the llm are mixed in with the ones the user had previously
/// if there isn't a blunderbuss commit the status of the commit will be unsuccessful, but that's fine
async fn pop_checkpoint_commit() -> anyhow::Result<()> {
	tokio::process::Command::new("sh").arg("-c")
		.arg(r#"if [[ $(git log -1 --pretty=%s) == "--blunderbuss-checkpoint--" ]]; then git reset HEAD~1; fi"#)
		.output().await?.demand_status_success("failed to remove blunderbuss checkpoint")?;

	Ok(())
}
/// remove all changes blunderbuss made and get rid of the blunderbuss checkpoint commit if there is one
async fn hard_reset_and_pop_checkpoint_commit() -> anyhow::Result<()> {
	tokio::process::Command::new("sh").arg("-c")
		.arg(r#"git reset --hard HEAD && git clean -fd"#)
		.output().await?.demand_status_success("failed to reset blunderbuss's changes")?;

	pop_checkpoint_commit().await?;

	Ok(())
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
