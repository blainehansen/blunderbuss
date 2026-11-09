pub fn genai_error_is_retryable(e: &genai::Error) -> bool {
	// e.headers()

	if let Some(status) = e.status() {
		return status.is_server_error()
			|| status == http::StatusCode::TOO_MANY_REQUESTS
			|| status == http::StatusCode::REQUEST_TIMEOUT
	}
	return false
}

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
				self.insert(other);
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
