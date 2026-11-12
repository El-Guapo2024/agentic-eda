//! The git repository a board's folder is in, as `${VCSHASH}`, `${VCSSHORTHASH}` and the `@{vcs..}` functions read it ([`eda_model::text_vars::Vcs`]).
//!
//! kicad-cli evaluates them in the folder of the project it loads (`SETTINGS_MANAGER::LoadProject` sets `TEXT_EVAL_VCS::SetContextPath` to it), which is the
//! board's `.kicad/` folder of derived files: the repository is the one the board's folder is in, found by walking up (`GetRepositoryForFile`), and a file
//! named by `vcsfile..( "x" )` is relative to `.kicad/`. The studio's server asks the same repository the same questions, through the `git` program (KiCad
//! links libgit2): it lists the commits and tags and walks them as KiCad 25ec2335's `text_eval_vcs.cpp` does (`GetFileCommit`, `ReadDescribeInfo`) --
//! not `git describe` or `git log -- file`, whose walks differ from KiCad's on a history that is not one line. The answers are kept for the one request (KiCad's `TEXT_EVAL::ENVIRONMENT` keeps them for one evaluation),
//! and a board in no repository, or a machine with no `git`, answers nothing, as KiCad's readers do outside a repository.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) struct GitRepo {
    /// The board's folder, where git is run (it exists; `.kicad/` may not yet).
    dir: PathBuf,
    asked: RefCell<HashMap<Vec<String>, String>>,
}

impl GitRepo {
    pub(crate) fn of_board(dir: &Path) -> GitRepo {
        GitRepo { dir: dir.to_path_buf(), asked: RefCell::new(HashMap::new()) }
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

    /// `git -C <board> <args>` fed `input` on its standard input: its output, untrimmed; empty when git fails.
    fn git_with_input(&self, args: &[&str], input: &str) -> String {
        use std::io::Write;
        let child = Command::new("git").arg("-C").arg(&self.dir).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = child else { return String::new() };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        match child.wait_with_output() {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => String::new(),
        }
    }

    /// `ResolveEffectivePath`: `"."` (or nothing) is the repository's HEAD; a relative file is the project folder's, made whole and normal
    /// (`wxFileName::MakeAbsolute`), in the board folder's real path, as git names the work tree (`ComputeSymlinkPreservingWorkDir`).
    fn file(&self, path: &str) -> Option<PathBuf> {
        if path.is_empty() || path == "." {
            return None;
        }
        let p = Path::new(path);
        let project = std::fs::canonicalize(&self.dir).unwrap_or_else(|_| self.dir.clone()).join(".kicad");
        let whole = if p.is_absolute() { p.to_path_buf() } else { project.join(p) };
        let mut out = PathBuf::new();
        for c in whole.components() {
            match c {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other),
            }
        }
        Some(out)
    }

    /// `GetFileCommit`: HEAD for `"."`; for a file, KiCad's walk of the history from HEAD in commit-time order (`GIT_SORT_TIME`): the first commit
    /// whose tree has the file, then on while each older commit has it with the same blob; the last of that run is the answer, so the walk stops at a
    /// commit where the file differs or is missing. A file outside the work tree, or in no commit, has none (empty).
    fn file_commit(&self, path: &str) -> String {
        let head = self.git(&["rev-parse", "--verify", "-q", "HEAD^{commit}"]);
        let Some(file) = self.file(path) else { return head };
        if head.is_empty() {
            return String::new();
        }
        let key = vec!["<file commit>".to_string(), file.to_string_lossy().into_owned()];
        if let Some(v) = self.asked.borrow().get(&key) {
            return v.clone();
        }
        let top = PathBuf::from(self.git(&["rev-parse", "--show-toplevel"]));
        let tree_path = match file.strip_prefix(&top) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
            _ => String::new(),
        };
        let mut result = String::new();
        if !tree_path.is_empty() {
            // `git_revwalk_sorting( walker, GIT_SORT_TIME )`: libgit2 limits the walk and sorts every commit by its time, newest first
            let mut commits: Vec<(i64, usize, String)> = self
                .git(&["log", "--format=%H %ct", "HEAD"])
                .lines()
                .enumerate()
                .filter_map(|(i, l)| l.split_once(' ').map(|(h, t)| (t.parse().unwrap_or(0), i, h.to_string())))
                .collect();
            commits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            // each commit's blob of the file (`git_tree_entry_bypath`), asked of `cat-file` at once
            let input: String = commits.iter().map(|(_, _, h)| format!("{h}:{tree_path}\n")).collect();
            let answers = self.git_with_input(&["cat-file", "--batch-check=%(objectname) %(objecttype)"], &input);
            let mut seen: Option<String> = None;
            for ((_, _, commit), line) in commits.iter().zip(answers.lines()) {
                let blob = (!line.ends_with(" missing")).then(|| line.split(' ').next().unwrap_or("").to_string());
                match (&seen, blob) {
                    (None, Some(b)) => {
                        seen = Some(b);
                        result = commit.clone();
                    }
                    (None, None) => {}
                    (Some(prev), Some(b)) if *prev == b => result = commit.clone(),
                    _ => break, // changed, or "doesn't exist in this commit, but existed before"
                }
            }
        }
        self.asked.borrow_mut().insert(key, result.clone());
        result
    }

    /// `git show -s --format=<format>` of the commit [`GitRepo::file_commit`] names; empty for none.
    fn commit_field(&self, path: &str, format: &str) -> String {
        let commit = self.file_commit(path);
        if commit.is_empty() {
            return String::new();
        }
        self.git(&["show", "-s", &format!("--format={format}"), &commit])
    }
}

/// `p_fnmatch( pattern, name, 0 )` (`git_tag_list_match`): `*` (also over `/`), `?`, `[..]` / `[!..]` with ranges, `\` quoting the next character.
fn fnmatch(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|i| fnmatch(&pattern[1..], &name[i..])),
        Some('?') => !name.is_empty() && fnmatch(&pattern[1..], &name[1..]),
        Some('[') => {
            let Some(&c) = name.first() else { return false };
            let mut i = 1;
            let negate = matches!(pattern.get(1), Some('!' | '^'));
            if negate {
                i += 1;
            }
            let mut matched = false;
            let start = i;
            while i < pattern.len() && (pattern[i] != ']' || i == start) {
                if pattern.get(i + 1) == Some(&'-') && pattern.get(i + 2).is_some_and(|e| *e != ']') {
                    matched |= pattern[i] <= c && c <= pattern[i + 2];
                    i += 3;
                } else {
                    matched |= pattern[i] == c;
                    i += 1;
                }
            }
            if i >= pattern.len() {
                // no closing bracket: a literal `[`
                return c == '[' && fnmatch(&pattern[1..], &name[1..]);
            }
            matched != negate && fnmatch(&pattern[i + 1..], &name[1..])
        }
        Some('\\') if pattern.len() > 1 => name.first() == Some(&pattern[1]) && fnmatch(&pattern[2..], &name[1..]),
        Some(p) => name.first() == Some(p) && fnmatch(&pattern[1..], &name[1..]),
    }
}

impl eda_model::text_vars::Vcs for GitRepo {
    fn commit_hash(&self, path: &str) -> String {
        self.file_commit(path)
    }

    /// `ReadDescribeInfo`: the tags matching `pattern` (`*` for none, `git_tag_list_match`) mapped to the commit each names -- an annotated tag peeled
    /// (`git_tag_peel`), a lightweight one only with `any_tags`, a later tag of one commit over an earlier one -- then the walk from HEAD in
    /// `GIT_SORT_TOPOLOGICAL | GIT_SORT_TIME` order (`git rev-list --date-order`): the first commit that has a tag, and how many came before it.
    fn describe(&self, pattern: &str, any_tags: bool) -> (String, i64) {
        let pattern: Vec<char> = if pattern.is_empty() { "*" } else { pattern }.chars().collect();
        if self.git(&["rev-parse", "--verify", "-q", "HEAD^{commit}"]).is_empty() {
            return (String::new(), 0);
        }
        let mut commit_to_tag: HashMap<String, String> = HashMap::new();
        let refs = self.git(&["for-each-ref", "refs/tags", "--format=%(refname:strip=2)%00%(objecttype)%00%(objectname)%00%(*objectname)"]);
        for line in refs.lines() {
            let f: Vec<&str> = line.split('\0').collect();
            let [name, kind, id, peeled] = f[..] else { continue };
            if !fnmatch(&pattern, &name.chars().collect::<Vec<_>>()) {
                continue;
            }
            match kind {
                // `git_tag_peel` to the first object that is not a tag
                "tag" => {
                    let target = self.git(&["rev-parse", "--verify", "-q", &format!("{id}^{{}}")]);
                    let target = if target.is_empty() { peeled.to_string() } else { target };
                    commit_to_tag.insert(target, name.to_string());
                }
                "commit" if any_tags => {
                    commit_to_tag.insert(id.to_string(), name.to_string());
                }
                _ => {}
            }
        }
        if commit_to_tag.is_empty() {
            return (String::new(), 0);
        }
        for (distance, commit) in self.git(&["rev-list", "--date-order", "HEAD"]).lines().enumerate() {
            if let Some(tag) = commit_to_tag.get(commit) {
                return (tag.clone(), distance as i64);
            }
        }
        (String::new(), 0)
    }

    fn is_dirty(&self, include_untracked: bool) -> bool {
        // `GIT_STATUS_OPT_INCLUDE_UNTRACKED` or else `GIT_STATUS_OPT_EXCLUDE_SUBMODULES`, the index and the work tree (`SHOW_INDEX_AND_WORKDIR`)
        let said = if include_untracked { self.git(&["status", "--porcelain", "--untracked-files=normal"]) } else { self.git(&["status", "--porcelain", "--untracked-files=no", "--ignore-submodules"]) };
        !said.is_empty()
    }

    fn signature(&self, path: &str, committer: bool, email: bool) -> String {
        let format = match (committer, email) {
            (false, false) => "%an",
            (false, true) => "%ae",
            (true, false) => "%cn",
            (true, true) => "%ce",
        };
        self.commit_field(path, format)
    }

    /// `KIGIT_COMMON::GetCurrentBranchName`: the branch HEAD resolves to; nothing when HEAD is detached (`git_branch_name` fails on it) or names no
    /// commit yet (`git_reference_resolve` fails).
    fn branch(&self) -> String {
        if self.git(&["rev-parse", "--verify", "-q", "HEAD^{commit}"]).is_empty() {
            return String::new();
        }
        self.git(&["symbolic-ref", "--short", "-q", "HEAD"])
    }

    fn commit_time(&self, path: &str) -> i64 {
        self.commit_field(path, "%ct").parse().unwrap_or(0)
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

    /// A merge of a branch tagged later than the line it joins: KiCad's walks (`ReadDescribeInfo`'s date order, `GetFileCommit`'s run of one blob in
    /// time order) answer what `git describe` and `git log -1 -- file` do not.
    #[test]
    fn a_merged_history_is_walked_as_kicads_text_evaluator_walks_it() {
        let root = std::env::temp_dir().join(format!("eda_vcs_merge_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let board = root.join("board");
        std::fs::create_dir_all(board.join(".kicad")).unwrap();
        let at = |args: &[&str], when: &str| {
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.com"])
                .args(args)
                .env("GIT_AUTHOR_DATE", when)
                .env("GIT_COMMITTER_DATE", when)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !at(&["init", "-q", "-b", "main"], "2026-11-01T00:00:00Z") {
            eprintln!("skipped: no git");
            return;
        }
        std::fs::write(board.join("design.json"), "1").unwrap();
        assert!(at(&["add", "."], "2026-11-01T00:00:00Z"));
        assert!(at(&["commit", "-q", "-m", "A"], "2026-11-01T00:00:00Z"));
        assert!(at(&["tag", "-a", "v1", "-m", "v1"], "2026-11-01T00:00:00Z"));
        let a = GitRepo::of_board(&board).commit_hash(".");
        // B on main changes the board; C on a side branch, newer, changes another file and is tagged v2
        assert!(at(&["checkout", "-q", "-b", "side"], "2026-11-01T00:00:00Z"));
        std::fs::write(root.join("notes.txt"), "x").unwrap();
        assert!(at(&["add", "."], "2026-11-03T00:00:00Z"));
        assert!(at(&["commit", "-q", "-m", "C"], "2026-11-03T00:00:00Z"));
        assert!(at(&["tag", "-a", "v2", "-m", "v2"], "2026-11-03T00:00:00Z"));
        assert!(at(&["checkout", "-q", "main"], "2026-11-03T00:00:00Z"));
        std::fs::write(board.join("design.json"), "2").unwrap();
        assert!(at(&["commit", "-q", "-am", "B"], "2026-11-02T00:00:00Z"));
        let b = GitRepo::of_board(&board).commit_hash(".");
        assert!(at(&["merge", "-q", "--no-ff", "-m", "M", "side"], "2026-11-04T00:00:00Z"));
        let g = GitRepo::of_board(&board);
        let m = g.commit_hash(".");
        assert!(m != a && m != b);
        // date order from M: M, C (v2), B, A -- one commit before v2; `git describe` counts M and B (v2-2)
        assert_eq!(g.describe("", false), ("v2".to_string(), 1));
        assert_eq!(g.describe("v1", false), ("v1".to_string(), 3));
        assert_eq!(g.describe("v[!2]*", false), ("v1".to_string(), 3));
        assert!(g.git(&["describe", "--long", "HEAD"]).starts_with("v2-2-"), "git's own walk differs");
        // the board in time order: M has "2", C has "1" -> the run of "2" ends at M (`git log -1 -- file` names B)
        assert_eq!(g.commit_hash("../design.json"), m);
        assert_eq!(g.git(&["log", "-1", "--format=%H", "--", "design.json"]), b);
        assert_eq!(g.commit_time("../design.json"), 1_793_750_400);
        assert_eq!(g.signature("../design.json", true, false), "Ada");
        // a lightweight tag counts only with any_tags
        assert!(at(&["tag", "light", &b], "2026-11-04T00:00:00Z"));
        let g = GitRepo::of_board(&board);
        assert_eq!(g.describe("l*", false), (String::new(), 0));
        assert_eq!(g.describe("l*", true), ("light".to_string(), 2));
        let _ = std::fs::remove_dir_all(&root);
    }
}
