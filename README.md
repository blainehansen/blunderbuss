# Klungo

## The LLM coding assistant that's actually an assistant

(meaning it shuts up and does what you say)

Klungo is the LLM coding assistant for engineers who

Cognition, understanding
Are correctly paranoid

- **Are sick of walls of slop.** A coding agent "talking" to you is completely unnecessary.
- **Recognize that LLMs are inherently unreliable and dangerous.**
- **Don't want to turn their brains off.**
- **Want to actually understand what they ship.**
- **Want to use cheap small private fast models.**

and so are allergic to yolo mode

Are sick of having to choose between the safety but irritation of manual mode vs the potentially production/life destroying risk but convenience of yolo mode
 to stay glued to their terminal to check and approve scary gobs of random python or bash
Are sick of irritating syncophantic bot speak
Aren't willing to accept a workflow that's just reading agonizing 10000 line prs
Don't want to read any more tedious walls of slop

## Philosophy


If you use the other coding agents out there, you will be forced to deal with a bunch of unnecessary dark patterns that distract you from the only thing that matters (the code!) and instead push you into cognitive surrender and skill atrophy. These dark patterns are intentional, since the companies have an incentive to make your psychology and productivity dependent on them. They don't have to design assistants like naggy slot machines, but they do anyway.

- **If you aren't running in yolo mode:** every 20 seconds or so you'll be nagged into giving permission to run a giant chunk of inscrutable bash or Python that is intentionally truncated and wrapped on one long line so you can't actually read and understand it.
- **If you are running in yolo mode:** you expose yourself to myriad dangers, and of course you've been pushed to give up trying to understand or monitor the agent. You have to spend a bunch of time and effort sandboxing these tools in order to be relatively safe, but of course they can still use whatever tools you give to mess you up if they go off the rails.
- Even after the task you wanted completed is complete, the agent will nag you to consider 5 more things, and the chat window stays open even if it isn't needed anymore.
- Every request you make prompts a giant wall of text that distracts you from the code and exposes you to all the potential toxicity of trend slop and persuasion bombing and hallucination fatigue and gaslighting and sycophancy induced psychosis
- By default you write in their tui, even though you're used to writing in your editor and are better and faster at doing so there. Pulling you away from your editor further pulls you away from the code and the workflow you've successfully used for years to build things

LLMs are a cognitive minefield. If we decide to use LLMs at all, we have to design their harnesses to completely remove or mitigate the dangers





LLM generated text is inherently a cognitive hazard. Being exposed to it for long periods mean you're drinking a tedious firehose of [bland stultifying mediocrity](https://archive.is/OPObK) ("trendslop") or [active manipulation](https://www.hbs.edu/ris/Publication%20Files/26-021_8db29bd1-04ed-4e98-86de-d286721afc7a.pdf) ("persuasion bombing"). There are very few situations where being exposed to LLM generated text is necessary or useful.

Reading large amounts of LLM generated text is a waste of time. By mathematical definition LLM text has either extremely low information density, or is a wall of pointless detail and jargon intended to wear you down and make you stop thinking.

The frontier labs have an incentive to design their coding assistants to trick you into [cognitive surrender](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=6097646) and increase your dependence on their tools. The more you give up thinking the more you lose the skills to perform your work without their coding assistants, and the more your system [inflates and accretes](https://bcantrill.dtrace.org/2026/04/12/the-peril-of-laziness-lost/) such that you couldn't work with it even if you still had the skill to.

https://www.anthropic.com/research/AI-assistance-coding-skills

I'm perfectly willing to use an LLM assistant that's *actually* an assistant, and so won't intentionally push gobs of irritating text at me or keep me glued to the terminal instead of working with code in my editor like I'm supposed to.

Slop fatigue
Permission fatigue
Cognitive surrender
automation bias
Reversion to the mean

https://pluralistic.net/2024/10/30/a-neck-in-a-noose/
https://papers.ssrn.com/sol3/papers.cfm?abstract_id=3921216


I'm trying to make an LLM coding assistant for [centaurs rather than reverse centaurs]().

https://codeberg.org/delightful-commons/delightful-humane-design




- The writing and reading you do should stay in your editor. Being in the editor keeps you engaged with the thing that actually matters: the code! Anyone who thinks they can actually maintain true understanding by having a "discourse" with a stochastic token predictor that routinely hallucinates is fooling themselves. The only LLM generated text you should read should be the actual code you're going to allow into your system. For this reason `klungo` always takes its instructions from `klungo.local.md` (you should add `*.local.*` to your `.gitignore`), and the reasoning and other text outputs aren't streamed to the terminal session but just dumped to a file so you'll only see them if you want to.
- It should always remain easy to just say "nope" and in one step revert all the changes the LLM made and either try again or just not bother. So once `klungo` is finished working you will simply be presented with a simple selection menu: `looks good` resets the temporary commit, blending your existing changes with the ones `klungo` made, then exits `klungo` entirely, leaving you to get back to the actual cognitive work of coding; `nope revert` just undoes all `klungo` changes and again resets the temporary commit, putting you exactly back where you started without having to make a checkpoint commit yourself; `not quite` reads the next chunk from `klungo.local.md` and uses it to continue working from the same point, again with a temporary commit to isolate the things `klungo` did on the last turn.
- You should remain fully in control of your version control history, and the fact that you used some assistant to make certain code changes is irrelevant to the version control history. `klungo` *can't* make git commits unless you define a tool that allows it to do so, and the `klungo` cli itself uses a temporary git commit to ensure you can fully review only the things klungo did in isolation.
- All tool use should be safe. `klungo` is intentionally very underpowered, and every action it takes is either easy to revert using version control, or explicitly allowed by you.
- `klungo` is only allowed to do things it's explicitly allowed to do: it will *never* prompt you to take actions that it isn't already simply empowered to take. The annoying "permission fatigue" is inherently a dark pattern: it forces users to either stay glued to their machine vigilantly checking every suggestion in order to remain safe, go to great lengths to sandbox the LLM, or put themselves in danger with "yolo mode." Designing an assistant that way is disrespectful and toxic and completely unnecessary.

## Usage

Install using `cargo`:

```sh
cargo install klungo
```

Klungo always reads `klungo.local.md` to get its instructions, and is invoked as `klg`.

```sh
echo "Fix the tests" > klungo.local.md
klg
# klungo works for a while
# then a simple selection menu appears asking
> nope revert
> looks good
> not quite
```

Klungo is intentionally designed in a "one-shot or nothing" way. We're trying to avoid the "futzing factor" as much as possible.

### Text commands

These text commands allow you to specify meta things about your task as you write instructions in `klungo.local.md`.

- `/check[<shell command>]`: you can specify a "check" shell command that must pass before klungo will consider the task done. The check command is provided as a tool klungo can call at will, and the check command will be run explicitly by the harness and must pass before the harness will stop. If the check command continues to fail then klungo will eventually reach `maxturns` and will fail overall.
- `/maxturns[<turn count>]`: sets the maximum number of "turns" before timeout. The default is intentionally very low, 20.
- `/tool[<shell command prefix>]`: you can give klungo access to whatever shell commands you trust, and it will be allowed to call the given prefix with any further text.
- `/allow|disallow/read|write[<file or dir glob>]`: overrides klungo's specific file permissions for this task, for example to disallow writing to the tests directory use `/disallow/write[tests/*]`. By default klungo isn't allowed to read or write files that aren't tracked by git, but all others are allowed. `/allow/write` implies `/allow/read`, and `/disallow/read` implies `/disallow/write`.
- `/model[<model qualifier>]`: allows you to specify what model to use for this task, using the [qualifier format used by the `genai` library](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution). Your api key will have to be available at whatever environment variable matches with the provider implied by your qualifier. You can also use your configured [model nicknames](#model-nicknames).

<!-- - `/allow_web(<url prefix>)`: by default klungo can't read any web urls at all, but you can specifically allow particular ones. -->
<!-- - `/worktree[<worktree slug>]`: creates and works in a new worktree in a sibling directory to this one, named `{current-dir}-{worktree-slug}`. The harness will prompt you  -->

### Global configuration

Klungo is intended to be a purely optional tool for individuals to choose as they desire, so for this reason all configuration is intended to be either at the user level or in purely ephemeral/untracked files at the project level, rather than permanent files at the project level.

Since

`~/.klungo.config.toml`

```toml
default_model = "opencode_go::glm-5.3-flash"
default_maxturns = 100

[[tool]]

decision_menu = ['not quite', 'nope revert', 'looks good']

[permissions.allow]
- tool(cargo check)
- tool(pnpm run test)
- web_read(http://localhost)
- web_read(https://crates.io/)

[permissions.disallow]
- read(*.env)

[model_nicknames]
slow = "opencode_go::glm-5.3"
medium = "opencode_go::glm-5.3-flash"
fast = "opencode_go::deepseek-v4.1-flash"
free = "opencode_go::beam-free"
```

There isn't a configuration option for "default check command," since each project and even task will tend to have a different one. If you tend to use the same `/check` over and over in a project, just keep it as the first thing in your `klungo.local.md` file. The klungo file sticks around from task to task!



### Model resolution and auth reference

| Provider prefix | API key env var |
|---|---|
| `openai::` | `OPENAI_API_KEY` |
| `openai_resp::` | `OPENAI_API_KEY` |
| `anthropic::` | `ANTHROPIC_API_KEY` |
| `gemini::` | `GEMINI_API_KEY` |
| `gemini_ix::` | `GEMINI_API_KEY` |
| `omlx::` | `OMLX_API_KEY` — optional; requires `OMLX_ENDPOINT` [github](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution)) |
| `ollama::` | None by default — local Ollama is unauthenticated [github](https://github.com/jeremychone/rust-genai/blob/main/examples/c00-readme.rs) |
| `ollama_cloud::` | `OLLAMA_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `vertex::` | `VERTEX_API_KEY`; must contain an OAuth2 bearer token. `VERTEX_PROJECT_ID` is also used for the endpoint. [github](https://github.com/jeremychone/rust-genai/blob/576b0c05/src/adapter/adapters/vertex/adapter_impl.rs) |
| `bedrock_api::` | `BEDROCK_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `bedrock_sigv4::` | No API-key variable; uses the AWS credential chain, with `AWS_PROFILE` optionally selecting the profile. [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `github_copilot::` | `GITHUB_TOKEN` |
| `opencode_go::` | `OPENCODE_GO_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `groq::` | `GROQ_API_KEY` |
| `together::` | `TOGETHER_API_KEY` |
| `fireworks::` | `FIREWORKS_API_KEY` |
| `cohere::` | `COHERE_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/examples/c00-readme.rs) |
| `nebius::` | `NEBIUS_API_KEY` |
| `mimo::` | `MIMO_API_KEY` |
| `deepseek::` | `DEEPSEEK_API_KEY` |
| `mistral::` | `MISTRAL_API_KEY` |
| `minimax::` | `MINIMAX_API_KEY` [deepwiki](https://deepwiki.com/jeremychone/rust-genai/3.1-adapter-system) |
| `zai::` | `ZAI_API_KEY` |
| `zai_coding::` | `ZAI_API_KEY` |
| `bigmodel::` | `BIGMODEL_API_KEY` |
| `aliyun::` | `ALIYUN_API_KEY` |
| `qwen_cloud::` | `QWEN_CLOUD_API_KEY` [github](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution)) |
| `baidu::` | `BAIDU_API_KEY` |
| `moonshot::` | `MOONSHOT_API_KEY` |
| `kimi::` | `KIMI_API_KEY` |
| `aihubmix::` | `AIHUBMIX_API_KEY` |
| `open_router::` | `OPEN_ROUTER_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `requesty::` | `REQUESTY_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `api_route::` | `API_ROUTE_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `heabsy::` | `HEABSY_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `runcrate::` | `RUNCRATE_API_KEY` [runcrate](https://www.runcrate.ai/docs/sdks/langchain) |
| `cloudgpu::` | `CLOUDGPU_API_KEY` [github](https://github.com/jeremychone/rust-genai/blob/main/docs/for-llm/api-reference-for-llm.md) |
| `cheapaimodel::` | `CHEAPAIMODEL_API_KEY` [cheapaimodel](https://cheapaimodel.ai/docs/guides/curl) |
| `atlascloud::` | `ATLASCLOUD_API_KEY` |
| `xai::` | `XAI_API_KEY` |

<!-- | `genai_1::`, `genai_2::`, … | `GENAI_1_API_KEY`, `GENAI_2_API_KEY`, …; generally `GENAI_{n}_API_KEY`. Also requires `GENAI_{n}_ENDPOINT`. [github](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution](https://github.com/jeremychone/rust-genai#model-to-adapter-resolution)) | -->
