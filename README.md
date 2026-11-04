# Klungo

## The LLM coding assistant that's actually an assistant

(meaning it shuts its mouth and does what you tell it)

## Philosophy

LLM generated text is inherently a cognitive hazard. Being exposed to it for long periods mean you're drinking a tedious firehose of [bland stultifying mediocrity](https://archive.is/OPObK) ("trendslop") or [active manipulation](https://www.hbs.edu/ris/Publication%20Files/26-021_8db29bd1-04ed-4e98-86de-d286721afc7a.pdf) ("persuasion bombing"). There are very few situations where being exposed to LLM generated text is necessary or useful.

I'm perfectly willing to use an LLM assistant that's *actually* an assistant, and so won't intentionally push gobs of irritating text at me or keep me glued to the terminal instead of working with code in my editor like I'm supposed to.

So Klungo is designed with a couple

- The writing and reading you do should stay in your editor. Being in the editor keeps you engaged with the thing that actually matters: the code! Anyone who thinks they can actually maintain true understanding by having a "discourse" with a stochastic token predictor that routinely hallucinates is fooling themselves. The only LLM generated text you should read should be the actual code you're going to allow into your system. For this reason `klungo` always takes its instructions from `klungo.local.md` (you should add `*.local.*` to your `.gitignore`), and the reasoning and other text outputs aren't streamed to the terminal session but just dumped to a file so you'll only see them if you want to.
- It should always remain easy to just say "nope" and in one step revert all the changes the LLM made and either try again or just not bother. So once `klungo` is finished working you will simply be presented with a simple selection menu: `looks good` resets the temporary commit, blending your existing changes with the ones `klungo` made, then exits `klungo` entirely, leaving you to get back to the actual cognitive work of coding; `nope revert` just undoes all `klungo` changes and again resets the temporary commit, putting you exactly back where you started without having to make a checkpoint commit yourself; `not quite` reads the next chunk from `klungo.local.md` and uses it to continue working from the same point, again with a temporary commit to isolate the things `klungo` did on the last turn.
- You should remain fully in control of your version control history, and the fact that you used some assistant to make certain code changes is irrelevant to the version control history. `klungo` *can't* make git commits unless you define a tool that allows it to do so, and the `klungo` cli itself uses a temporary git commit to ensure you can fully review only the things klungo did in isolation.
- All tool use should be safe. `klungo` is intentionally very underpowered, and every action it takes is either easy to revert using version control, or explicitly allowed by you.
- `klungo` is only allowed to do things it's explicitly allowed to do: it will *never* prompt you to take actions that it isn't already simply empowered to take. The annoying "permission fatigue" is inherently a dark pattern: it forces users to either stay glued to their machine vigilantly checking every suggestion in order to remain safe, go to great lengths to sandbox the LLM, or put themselves in danger with "yolo mode." Designing an assistant that way is disrespectful and toxic and completely unnecessary.

## Usage

Klungo always reads `klungo.local.md` to get its instructions.

```sh
echo "Fix the tests" > klungo.local.md
klg
# klungo works for a while
# then a simple selection menu appears asking
> nope revert
> looks good
> not quite
```

<!--
is it overkill to have a stack? maybe not quite is even not something for the first version
-->

## Installation

Right now klungo can only be installed using `cargo`:

```sh
cargo install klungo
```
