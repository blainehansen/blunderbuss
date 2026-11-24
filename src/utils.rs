use anyhow::Context;

pub trait ResultExt<T> {
	fn into_ok_or_err(self) -> T;
}

impl<T> ResultExt<T> for Result<T, T> {
	fn into_ok_or_err(self) -> T {
		let (Ok(i) | Err(i)) = self; i
	}
}

// pub trait OptionExt<T> {
// 	fn try_insert(&mut self, other: T) -> Result<(), &T>;
// }

// impl<T> OptionExt<T> for Option<T> {
// 	fn try_insert(&mut self, other: T) -> Result<(), &T> {
// 		match self {
// 			Some(value) => Err(value),
// 			None => {
// 				let _ = self.insert(other);
// 				Ok(())
// 			},
// 		}
// 	}
// }

pub fn json_err(message: impl ToString) -> serde_json::Value {
	serde_json::json!({ "error": message.to_string() })
}

pub fn default_dot() -> std::path::PathBuf {
	std::path::PathBuf::from(".")
}

#[derive(Debug)]
pub struct PathEnv {
	cur: std::path::PathBuf,
	gignore: ignore::gitignore::Gitignore,
	// TODO when doing a read operation, yes all allow_read and allow_write, no (invert) all disallow_read
	// TODO when doing a write operation, yes all allow_write, no (invert) all disallow_read and disallow_write

	// allow_read: Option<Vec<String>>,
	// allow_write: Option<Vec<String>>,
	// disallow_read: Option<Vec<String>>,
	// disallow_write: Option<Vec<String>>,
}

impl PathEnv {
	pub fn new() -> anyhow::Result<PathEnv> {
		let cur = std::env::current_dir()
			.with_context(|| format!("wasn't able to get the current directory, which is needed to validate tool paths"))?;

		let mut ig = ignore::gitignore::GitignoreBuilder::new(".");
		ig.add(".gitignore");
		// ig.add_line(None, line);
		let gignore = ig.build().unwrap_or_else(|_| ignore::gitignore::Gitignore::empty());

		Ok(PathEnv { cur, gignore })
	}

	fn demand_path_is_relative<'p>(&self, path: &'p std::path::Path) -> Result<&'p std::path::Path, String> {
		if !path.has_root() {
			return
				if !path.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
					Err(format!("path must be relative and simple: {}", path.display()))
				}
				else { Ok(path) }
		}

		path.strip_prefix(&self.cur)
			.map_err(|e| format!("path isn't a child of the current working directory {}: {e}", path.display()))
	}

	// https://github.com/BurntSushi/ripgrep/tree/master/crates/globset
	// TODO these methods need to also accept a PermKind to evaluate whether the current action is allowed
	pub fn file_is_allowed(&self, path: &std::path::Path) -> Result<(), String> {
		let path = self.demand_path_is_relative(path)?;

		(!self.gignore.matched_path_or_any_parents(path, false).is_ignore())
			.ok_or_else(|| format!("file {} is ignored, meaning it is off limits", path.display()))
	}

	pub fn dir_is_allowed(&self, path: &std::path::Path) -> Result<(), String> {
		let path = self.demand_path_is_relative(path)?;

		(!self.gignore.matched_path_or_any_parents(path, true).is_ignore())
			.ok_or_else(|| format!("dir {} is ignored, meaning it is off limits", path.display()))
	}
}



#[test]
fn test_file_is_allowed_yes() {
	let path_env = PathEnv::new().unwrap();
	assert!(path_env.file_is_allowed(std::path::Path::new("Cargo.toml")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("./src/main.rs")).is_ok());
	assert!(path_env.file_is_allowed(std::path::Path::new("src/main.rs")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("src/./stuff/./hello")).is_ok());

	assert!(path_env.file_is_allowed(std::path::Path::new("/home/blaine/lab/blunderbuss/src/main.rs")).is_ok());

	assert!(path_env.file_is_allowed(std::path::Path::new("something/cool/that/i/do/data.json")).is_ok());
}

#[test]
fn test_file_is_allowed_no() {
	let path_env = PathEnv::new().unwrap();
	assert!(path_env.file_is_allowed(std::path::Path::new("dev.local.sh")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("secret.local.env")).is_err());

	assert!(path_env.file_is_allowed(std::path::Path::new("target")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("target/release")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("target/release/blunderbuss")).is_err());
}

#[test]
fn test_file_is_allowed_outside() {
	let path_env = PathEnv::new().unwrap();
	assert!(path_env.file_is_allowed(std::path::Path::new("../../not/where/we/are")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("../../not/where/we/are.json")).is_err());

	assert!(path_env.file_is_allowed(std::path::Path::new("/home/blunderbuss/not/where/we/are")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("/home/blunderbuss/not/where/we/are.json")).is_err());
}

#[test]
fn test_dir_is_allowed_yes() {
	let path_env = PathEnv::new().unwrap();
	assert!(path_env.dir_is_allowed(std::path::Path::new(".")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("./src")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("src/./stuff/./hello")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("src")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("something/cool/that/i/do/data")).is_ok());

	assert!(path_env.file_is_allowed(std::path::Path::new("/home/blaine/lab/blunderbuss/src")).is_ok());

	assert!(path_env.dir_is_allowed(std::path::Path::new("Cargo.toml")).is_ok());
	assert!(path_env.dir_is_allowed(std::path::Path::new("src/main.rs")).is_ok());
}

#[test]
fn test_dir_is_allowed_no() {
	let path_env = PathEnv::new().unwrap();
	assert!(path_env.dir_is_allowed(std::path::Path::new("db.local.data")).is_err());
	assert!(path_env.dir_is_allowed(std::path::Path::new("target")).is_err());
	assert!(path_env.dir_is_allowed(std::path::Path::new("target/release")).is_err());

	assert!(path_env.dir_is_allowed(std::path::Path::new("dev.local.sh")).is_err());
	assert!(path_env.dir_is_allowed(std::path::Path::new("secret.local.env")).is_err());
}

#[test]
fn test_dir_is_allowed_outside() {
	let path_env = PathEnv::new().unwrap();
	assert!(path_env.file_is_allowed(std::path::Path::new("../../not/where/we/are")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("../../not/where/we/are.json")).is_err());

	assert!(path_env.file_is_allowed(std::path::Path::new("/home/blunderbuss/not/where/we/are")).is_err());
	assert!(path_env.file_is_allowed(std::path::Path::new("/home/blunderbuss/not/where/we/are.json")).is_err());
}


pub trait OutputExt {
	fn demand_status_success(self, ctx: &str) -> anyhow::Result<std::process::Output>;
}

impl OutputExt for std::process::Output {
	fn demand_status_success(self, ctx: &str) -> anyhow::Result<std::process::Output> {
		if !self.status.success() {
			let stdout = String::from_utf8_lossy(&self.stdout);
			let stderr = String::from_utf8_lossy(&self.stderr);
			return Err(anyhow::anyhow!("{ctx}\nstdout:\n{stdout}\n\nstderr:\n{stderr}"))
		}
		Ok(self)
	}
}


pub struct StrCursor<'a> {
	rest: &'a str,
}

impl<'a> StrCursor<'a> {
	pub fn new(input: &'a str) -> Self {
		Self { rest: input }
	}

	pub fn remaining(&self) -> &'a str {
		self.rest
	}
	pub fn into_remaining(self) -> &'a str {
		self.rest
	}

	pub fn advance_to(&mut self, target: &str) -> Option<&'a str> {
		let idx = self.rest.find(target)?;
		let (skipped, remaining) = self.rest.split_at(idx);
		self.rest = remaining;
		Some(skipped)
	}

	pub fn advance_past(&mut self, target: &str) -> Option<&'a str> {
		let idx = self.rest.find(target)?;
		let skipped = &self.rest[..idx + target.len()];
		self.rest = &self.rest[idx + target.len()..];
		Some(skipped)
	}
}

#[test]
fn test_str_cursor() {
	let mut cursor = StrCursor::new("aaa bbb aaa ccc");
	assert_eq!(cursor.remaining(), "aaa bbb aaa ccc");

	assert_eq!(cursor.advance_to("").unwrap(), "");
	assert_eq!(cursor.remaining(), "aaa bbb aaa ccc");

	assert_eq!(cursor.advance_past("").unwrap(), "");
	assert_eq!(cursor.remaining(), "aaa bbb aaa ccc");

	assert!(cursor.advance_to("nope").is_none());

	assert_eq!(cursor.advance_to("bbb").unwrap(), "aaa ");
	assert_eq!(cursor.remaining(), "bbb aaa ccc");

	assert!(cursor.advance_past("blah").is_none());

	assert_eq!(cursor.advance_past("aaa").unwrap(), "bbb aaa");
	assert_eq!(cursor.remaining(), " ccc");

	let mut cursor = StrCursor::new("aaa bbb aaa ccc");
	assert_eq!(cursor.advance_past("ccc").unwrap(), "aaa bbb aaa ccc");
	assert_eq!(cursor.remaining(), "");
}
