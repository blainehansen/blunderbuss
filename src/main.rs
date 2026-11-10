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
		res = tokio::signal::ctrl_c() => {
			match res {
				Ok(()) => println!("Ctrl+C received! Dropping main program future..."),
				Err(err) => eprintln!("Failed to listen for Ctrl+C: {}", err),
			}
		},
		chat_res = chat_loop() => {
			match chat_res {
				Ok(()) => println!("Ctrl+C received! Dropping main program future..."),
				Err(err) => eprintln!("Failed to listen for Ctrl+C: {}", err),
			}
		},
	}

	Ok(())
}

async fn create_new_chat() -> anyhow::Result<(ChatParams, genai::chat::ChatRequest)> {
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

	let other_tools = default_tools.unwrap_or_default().into_iter().chain(instruction_tools.unwrap_or_default().into_iter())
		.map(|m| tools::tool_general_shell(m.as_str())).collect::<Vec<_>>();

	let mut all_tools = [
		// tool_tree(),
		tools::tool_grep(),
		tools::tool_read_file(),
		tools::tool_create_file(),
		tools::tool_write_chunk(),
	].into_iter()
		.chain(other_tools.clone())
		.collect::<Vec<_>>();

	if let Some(check_command) = &check_command {
		all_tools.push(tools::tool_check_command(check_command));
	}

	let other_tools = other_tools.into_iter()
		.map(|t| (t.name.to_string(), t)).collect::<HashMap<_, _>>();


	println!("using model {model}");
	println!("using maxturns {maxturns}");
	if let Some(check_command) = &check_command {
		println!("using check command {check_command}");
	}

	let mut chat_req = genai::chat::ChatRequest::default()
		.with_system(include_str!("./system.md"))
		.with_tools(all_tools);

	chat_req = chat_req.append_message(genai::chat::ChatMessage::user(text));

	let instructions = ChatParams { model, maxturns, other_tools, check_command };

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

#[derive(Debug)]
struct ChatParams {
	model: String,
	maxturns: usize,
	other_tools: HashMap<String, genai::chat::Tool>,
	check_command: Option<String>,
}


async fn chat_loop() -> anyhow::Result<()> {
	let (mut p, mut chat_req) = create_new_chat().await?;

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
			let output = tools::run_tool(&tool_call, p.check_command.as_deref(), &p.other_tools).await;

			let tool_response = genai::chat::ToolResponse::from_tool_call(&tool_call, output.to_string());
			tool_responses.push(tool_response);
		}
		// append this round to the history
		chat_req = chat_req.append_message(
			genai::chat::ChatMessage::assistant(content)
				.with_reasoning_content(reasoning_content)
		);

		// the llm is still doing things, so keep at it
		if !tool_responses.is_empty() {
			chat_req = chat_req.append_message(tool_responses);
			turn += 1;
			continue
		}
		// there aren't any more calls (and therefore any more responses), the llm says its done
		else {
			if let Some(check_command) = &p.check_command {
				println!("klungo says it's finished, running your check command {check_command}");
				let output = tokio::process::Command::new("sh").arg("-c").arg(check_command)
					.output().await.with_context(|| format!("wasn't able to call sh -c '{check_command}'"))?;

				if !output.status.success() {
					let stdout = String::from_utf8_lossy(&output.stdout);
					let stderr = String::from_utf8_lossy(&output.stderr);
					println!("check command {check_command} failed! sending the output back to klungo to try again");
					let check_command_feedback = format!("the check command {check_command} failed.\nstdout:\n{stdout}\n\n\nstderr:\n{stderr}");
					chat_req = chat_req.append_message(genai::chat::ChatMessage::user(check_command_feedback));

					turn += 1;
					continue
				}
				println!("check command {check_command} succeeded!");
			}

			const LOOKS_GOOD: &'static str = "looks good";
			const NOPE_REVERT: &'static str = "nope revert";
			const TRY_AGAIN_ANEW: &'static str = "try again (start new chat from klungo.local.md)";
			const TRY_AGAIN_FROM_HERE: &'static str = "try again (continue this chat)";
			let options: Vec<&str> = vec![LOOKS_GOOD, NOPE_REVERT, TRY_AGAIN_FROM_HERE];
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
					(p, chat_req) = create_new_chat().await?;
					turn = 0;
					continue;
				},
				TRY_AGAIN_FROM_HERE => {
					// this tries again with the context we already have
					hard_reset_checkpoint_commit().await?;
					turn = 0;
					chat_req = chat_req.append_message(genai::chat::ChatMessage::user(
						"I'm unsatisfied with that attempt. everything has been reverted, try again",
					));
					continue;
				},
				unknown => { return Err(anyhow::anyhow!("how did you choose this? {unknown}")) }
			}
		}
	}

	// chat_req.messages.into_iter()
	let chat_json = serde_json::to_vec_pretty(&chat_req.messages)?;
	tokio::fs::write("klungo.output.local.json", chat_json).await?;

	if ran_out_of_turns {
		return Err(anyhow::anyhow!("klungo failed to finish within the max turns: {}", p.maxturns))
	}

	Ok(())
}

// https://itnext.io/multitask-like-a-pro-with-the-wip-commit-2f4d40ca0192
async fn push_checkpoint_commit() -> anyhow::Result<()> {
	let output = tokio::process::Command::new("sh").arg("-c")
		.arg("git add -A; git rm $(git ls-files --deleted) 2> /dev/null; git commit --no-verify --no-gpg-sign --message '--klungo-checkpoint--'")
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
