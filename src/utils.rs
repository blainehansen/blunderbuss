pub trait ResultExt<T> {
	fn into_ok_or_err(self) -> T;
}

impl<T> ResultExt<T> for Result<T, T> {
	fn into_ok_or_err(self) -> T {
		let (Ok(i) | Err(i)) = self; i
	}
}

pub trait OptionExt<T> {
	fn try_insert(&mut self, other: T) -> Result<(), &T>;
}

impl<T> OptionExt<T> for Option<T> {
	fn try_insert(&mut self, other: T) -> Result<(), &T> {
		match self {
			Some(value) => Err(value),
			None => {
				let _ = self.insert(other);
				Ok(())
			},
		}
	}
}

pub fn json_err(message: impl ToString) -> serde_json::Value {
	serde_json::json!({ "error": message.to_string() })
}

pub fn default_dot() -> std::path::PathBuf {
	std::path::PathBuf::from(".")
}

// https://github.com/BurntSushi/ripgrep/tree/master/crates/globset
pub fn path_is_allowed_based_on_gitignore(path: &std::path::Path) -> Result<(), String> {
	let (ig, _) = ignore::gitignore::Gitignore::new(".");

	ig.matched(path, false).is_ignore()
		.ok_or_else(|| format!("file {} is ignored, meaning it is off limits", path.display()))
}

pub fn dir_is_allowed_based_on_gitignore(path: &std::path::Path) -> Result<(), String> {
	let (ig, _) = ignore::gitignore::Gitignore::new(".");

	ig.matched(path, true).is_ignore()
		.ok_or_else(|| format!("dir {} is ignored, meaning it is off limits", path.display()))
}

pub fn demand_command_success(output: std::process::Output, ctx: &str) -> anyhow::Result<()> {
	if !output.status.success() {
		let stdout = String::from_utf8_lossy(&output.stdout);
		let stderr = String::from_utf8_lossy(&output.stderr);
		return Err(anyhow::anyhow!("{ctx}\nstdout:\n{stdout}\n\nstderr:\n{stderr}"))
	}
	Ok(())
}
