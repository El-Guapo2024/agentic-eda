//! Import Sheet: the studio's server reads the schematic file, the `ImportSheet` verb reads none.
//!
//! A command has to replay and undo the same on a machine that lacks the file (`board::step_quiet` stores snapshots, the activity log
//! prints the command), so `Cmd::ImportSheet { source }` is rewritten before it is applied: the file at `source` -- an absolute path, or one
//! relative to the board's directory -- is read, with the sheets it places and the library symbols it embeds, into the command's `content`
//! (`eda_kicad::load_sheet_file`, `SCH_EDIT_FRAME::LoadSheetFromFile`'s reading). A command that carries its content already is left alone.

use eda_model::CheckResult;
use eda_ops::Cmd;
use std::path::{Path, PathBuf};

/// Where a path the user gave points: as it is when absolute, else inside the board's directory.
pub(crate) fn resolve(dir: &Path, source: &str) -> PathBuf {
    let p = Path::new(source.trim());
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

/// `cmd` with the file of every `ImportSheet` in it read, or `None` when there is nothing to read.
pub(crate) fn with_loaded_file(cmd: &Cmd, dir: &Path) -> Result<Option<Cmd>, Vec<CheckResult>> {
    match cmd {
        Cmd::Batch { cmds } => {
            let mut changed = false;
            let mut out = Vec::with_capacity(cmds.len());
            for c in cmds {
                match with_loaded_file(c, dir)? {
                    Some(c) => {
                        changed = true;
                        out.push(c);
                    }
                    None => out.push(c.clone()),
                }
            }
            Ok(changed.then_some(Cmd::Batch { cmds: out }))
        }
        Cmd::OnSheet { sheet, cmd } => Ok(with_loaded_file(cmd, dir)?.map(|inner| Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(inner) })),
        Cmd::ImportSheet { source, content: None, .. } => {
            let loaded = eda_kicad::load_sheet_file(&resolve(dir, source))?;
            let mut read = cmd.clone();
            if let Cmd::ImportSheet { content, .. } = &mut read {
                *content = Some(loaded);
            }
            Ok(Some(read))
        }
        _ => Ok(None),
    }
}
