use std::collections::HashMap;
use anyhow::Context;

mod tools;
mod utils;
mod parser;

type JsonResult = Result<serde_json::Value, String>;
const DEFAULT_MAXTURNS: usize = 20;

#[derive(Debug, Default, serde::Deserialize)]
struct Config {
	default_model: Option<String>,
	default_maxturns: Option<usize>,
	tools: Option<Vec<String>>,
}

// https://github.com/BurntSushi/ripgrep/tree/master/crates/globset

async fn try_klungo_config() -> anyhow::Result<Config> {
	let klungo_config_path = std::env::home_dir().unwrap_or("~/".into()).join(".klungo.config.yml");
	if !tokio::fs::try_exists(&klungo_config_path).await.unwrap_or(false) {
		return Ok(Config::default())
	}

	let file_contents = tokio::fs::read_to_string(klungo_config_path).await
		.context("wasn't able to read .klungo.config.yml even though it exists?")?;

	yaml_serde::from_str(&file_contents)
		.context("wasn't able to parse .klungo.config.yml")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	// tracing_subscriber::fmt()
	// 	.with_env_filter(EnvFilter::new("genai=debug"))
	// 	// .with_max_level(tracing::Level::DEBUG) // To enable all sub-library tracing
	// 	.init();

	let Config { default_model, default_maxturns, tools: default_tools } = try_klungo_config().await?;
	let instructions = tokio::fs::read_to_string("klungo.local.md").await
		// TODO allow piping stdin at some point
		.map_err(|e| anyhow::anyhow!("couldn't read the klungo.local.md file, which is the whole way klungo works!: {e}"))?;

	let parser::Instructions {
		tools: instruction_tools, model, allows, disallows, maxturns, text: instruction_text, check_command,
	} = parser::parse_instructions(&instructions)
		.map_err(|e| anyhow::anyhow!(e))?;

	let model = model
		.or(default_model)
		.ok_or_else(|| anyhow::anyhow!("you don't have a model specified! you have to either provide one using /model[<model qualifier>] in klungo.local.md or `default_model` in ~/.klungo.config.yml"))?;

	let maxturns = maxturns
		.or(default_maxturns)
		.unwrap_or(DEFAULT_MAXTURNS);

	// TODO add allow and disallow

	// let default_tools = default_tools.unwrap_or_default().into_iter().map(|t| tools::tool_general_shell(&t)).collect::<Vec<_>>();
	// let instruction_tools = instruction_tools.unwrap_or_default().into_iter().map(|m| tools::tool_general_shell(m.as_str())).collect::<Vec<_>>();

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

	// let client = genai::Client::builder().with_model_mapper_fn(|_model_iden| {
	// 	// Ok(genai::ModelIden::from_static(genai::adapter::AdapterKind::Gemini, "gemma-4-31b-it"))
	// 	Ok(genai::ModelIden::from_static(genai::adapter::AdapterKind::Gemini, "gemma-4-26b-a4b-it"))
	// }).build()?;

	let client = genai::Client::new()?;

	let mut chat_req = genai::chat::ChatRequest::default()
		.with_system(include_str!("./system.md"))
		.with_tools(all_tools);

	chat_req = chat_req.append_message(genai::chat::ChatMessage::user(instruction_text));

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

	let mut turn = 0;
	let mut ran_out_of_turns = false;
	loop {
		if turn >= maxturns {
			ran_out_of_turns = true;
			break
		}

		// execute this turn
		use backon::Retryable;
		let chat_res = (|| async { client.exec_chat(&model, chat_req.clone(), None).await })
			.retry(backon::ExponentialBuilder::default())
			.when(utils::genai_error_is_retryable)
			.notify(|err, dur| {
				println!("retrying error {:?} after {:?}", err, dur);
			})
			.await?;

		let genai::chat::ChatResponse { content, reasoning_content, .. } = chat_res;

		let tool_calls = content.tool_calls();
		let mut tool_responses = Vec::with_capacity(tool_calls.len());
		for tool_call in tool_calls.into_iter() {
			let output = tools::run_tool(&tool_call, check_command.as_deref(), &other_tools).await;

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
			if let Some(check_command) = &check_command {
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

			let options: Vec<&str> = vec!["looks good", "try again", "nope revert"];

			let choice = inquire::Select::new("How's that?", options)
				.with_page_size(3)
				.without_help_message()
				.prompt().context("there was some issue withe the inquire library?")?;

			match choice {
				"looks good" => {
					// pop the temp git commit
				},
				"nope revert" => {
					// revert and hard reset
					// pop the temp git commit
				},
				"try again" => {
					// this should reread the instructions, right?
					// ironically
				},
				unknown => { return Err(anyhow::anyhow!("how did you choose this? {unknown}")) }
			}

			// we end up here both if there isn't a check command or if the check command succeeded
			break;
		}
	}

	// chat_req.messages.into_iter()
	let chat_json = serde_json::to_vec_pretty(&chat_req.messages)?;
	tokio::fs::write("klungo.output.local.json", chat_json).await?;

	if ran_out_of_turns {
		return Err(anyhow::anyhow!("klungo failed to finish within the max steps of {maxturns}"))
	}

	Ok(())
}
