//! The git repository a board's folder is in, as `${VCSHASH}`, `${VCSSHORTHASH}` and the `@{vcs..}` functions read it ([`eda_model::text_vars::Vcs`]).
//!
//! kicad-cli evaluates them in the folder of the project it loads (`SETTINGS_MANAGER::LoadProject` sets `TEXT_EVAL_VCS::SetContextPath` to it), which is the
//! board's `.kicad/` folder of derived files: the repository is the one the board's folder is in, found by walking up (`GetRepositoryForFile`), and a file
//! named by `vcsfile..( "x" )` is relative to `.kicad/`. The studio's server asks the same repository the same questions, through the `git` program (KiCad
//! links libgit2; the questions are the same): the answers are kept for the one request (KiCad's `TEXT_EVAL::ENVIRONMENT` keeps them for one evaluation),
//! and a board in no repository, or a machine with no `git`, answers nothing, as KiCad's readers do outside a repository.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) struct GitRepo {
    /// The project's folder: the board's `.kicad/`.
    project_dir: PathBuf,
    /// The board's folder, where git is run (it exists; `.kicad/` may not yet).
    dir: PathBuf,
    asked: RefCell<HashMap<Vec<String>, String>>,
}

impl GitRepo {
    pub(crate) fn of_board(dir: &Path) -> GitRepo {
        GitRepo { project_dir: dir.join(".kicad"), dir: dir.to_path_buf(), asked: RefCell::new(HashMap::new()) }
    }

    /// `git -C <board> <args>`'s output, trimmed; empty when git fails (no repository, no such commit) or is not there.
    fn git(&self, args: &[&str]) -> String {
        let key: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        if let Some(v) = self.asked.borrow().get(&key) {
            return v.clone();
        }
        let out = Command::new("git").arg("-C").arg(&self.dir).args(args).output();
        let text = match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => String::new(),
        };
        self.asked.borrow_mut().insert(key, text.clone());
        text
    }

    /// `ResolveEffectivePath`: `"."` (or nothing) is the repository's HEAD; a relative file is the project folder's.
    fn file(&self, path: &str) -> Option<String> {
        if path.is_empty() || path == "." {
            return None;
        }
        let p = Path::new(path);
        Some(if p.is_absolute() { p.to_path_buf() } else { self.project_dir.join(p) }.to_string_lossy().into_owned())
    }

    /// `git log -1 --format=<format>` of HEAD, or of the last commit that changed the file.
    fn last_commit(&self, path: &str, format: &str) -> String {
        let format = format!("--format={format}");
        match self.file(path) {
            None => self.git(&["log", "-1", &format]),
            Some(f) => self.git(&["log", "-1", &format, "--", &f]),
        }
    }
}

impl eda_model::text_vars::Vcs for GitRepo {
    fn commit_hash(&self, path: &str) -> String {
        match self.file(path) {
            None => self.git(&["rev-parse", "--verify", "-q", "HEAD"]),
            Some(_) => self.last_commit(path, "%H"),
        }
    }

    fn describe(&self, pattern: &str, any_tags: bool) -> (String, i64) {
        let pattern = if pattern.is_empty() { "*" } else { pattern };
        let mut args = vec!["describe", "--long", "--abbrev=40"];
        if any_tags {
            args.push("--tags");
        }
        args.extend(["--match", pattern, "HEAD"]);
        let said = self.git(&args);
        // `<tag>-<distance>-g<hash>`; a tag may hold `-` itself
        let mut parts = said.rsplitn(3, '-');
        let (_hash, distance, tag) = (parts.next(), parts.next(), parts.next());
        match (tag, distance.and_then(|d| d.parse::<i64>().ok())) {
            (Some(tag), Some(d)) => (tag.to_string(), d),
            _ => (String::new(), 0),
        }
    }

    fn is_dirty(&self, include_untracked: bool) -> bool {
        let said = if include_untracked { self.git(&["status", "--porcelain", "--untracked-files=all"]) } else { self.git(&["status", "--porcelain", "--untracked-files=no", "--ignore-submodules"]) };
        !said.is_empty()
    }

    fn signature(&self, path: &str, committer: bool, email: bool) -> String {
        let format = match (committer, email) {
            (false, false) => "%an",
            (false, true) => "%ae",
            (true, false) => "%cn",
            (true, true) => "%ce",
        };
        self.last_commit(path, format)
    }

    fn branch(&self) -> String {
        self.git(&["symbolic-ref", "--short", "-q", "HEAD"])
    }

    fn commit_time(&self, path: &str) -> i64 {
        self.last_commit(path, "%ct").parse().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::text_vars::Vcs;

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git").arg("-C").arg(dir).args(args).env("GIT_AUTHOR_DATE", "2026-11-06T12:00:00Z").env("GIT_COMMITTER_DATE", "2026-11-06T12:00:00Z").output().is_ok_and(|o| o.status.success())
    }

    /// A board folder in a repository of two commits and a tag, and one in none.
    #[test]
    fn a_board_in_a_repository_answers_from_its_head_and_one_outside_answers_nothing() {
        let root = std::env::temp_dir().join(format!("eda_vcs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let board = root.join("repo").join("board");
        std::fs::create_dir_all(board.join(".kicad")).unwrap();
        let repo = root.join("repo");
        if !git(&repo, &["init", "-q", "-b", "main"]) {
            eprintln!("skipped: no git");
            return;
        }
        let id = ["-c", "user.name=Ada", "-c", "user.email=ada@example.com"];
        std::fs::write(board.join("design.json"), "{}").unwrap();
        assert!(git(&repo, &["add", "."]));
        assert!(git(&repo, &[id[0], id[1], id[2], id[3], "commit", "-q", "-m", "one"]));
        assert!(git(&repo, &[id[0], id[1], id[2], id[3], "tag", "-a", "v1", "-m", "v1"]));
        std::fs::write(board.join("design.json"), "{ }").unwrap();
        assert!(git(&repo, &[id[0], id[1], id[2], id[3], "commit", "-q", "-am", "two"]));
        let g = GitRepo::of_board(&board);
        let head = g.commit_hash(".");
        assert_eq!(head.len(), 40, "{head}");
        assert_eq!(g.describe("", false), ("v1".to_string(), 1));
        assert_eq!(g.describe("x*", false), (String::new(), 0));
        assert_eq!(g.branch(), "main");
        assert_eq!(g.signature(".", false, false), "Ada");
        assert_eq!(g.signature(".", true, true), "ada@example.com");
        assert_eq!(g.commit_time("."), 1_793_966_400);
        assert!(!g.is_dirty(false));
        std::fs::write(board.join("new.txt"), "x").unwrap();
        assert!(!g.is_dirty(false), "an untracked file is not counted unless asked");
        assert!(GitRepo::of_board(&board).is_dirty(true));
        // a file of the project folder is `.kicad/`'s; one never committed has no commit
        assert_eq!(g.commit_hash("../design.json"), head);
        assert_eq!(g.commit_hash("nope.kicad_sch"), "");
        // outside a repository
        let alone = root.join("alone");
        std::fs::create_dir_all(&alone).unwrap();
        let none = GitRepo::of_board(&alone);
        if none.commit_hash(".").is_empty() {
            assert_eq!(none.describe("", true), (String::new(), 0));
            assert_eq!(none.branch(), "");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
