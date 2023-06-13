pub(crate) use std::path::PathBuf;

use colored::Colorize;
use walkdir::WalkDir;

use crate::files::{ApplyResult, DotFile};

pub fn walk_config_dir(path: &PathBuf) -> impl Iterator<Item = walkdir::DirEntry> {
    // TODO: how does ripgrep handle this?
    let walker = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let entrystring = path.to_str().unwrap();
            !entrystring.ends_with(".imosid.toml")
                && !entrystring.contains("/.git/")
                && path.to_path_buf().is_file()
        });
    return walker;
}

pub fn walk_dotfiles(path: &PathBuf) -> Vec<DotFile> {
    let mut dotfiles = Vec::new();
    for entry in walk_config_dir(path) {
        let entrypath = entry.path().to_path_buf();
        let dotfile = match DotFile::from_pathbuf(&entrypath) {
            Ok(file) => file,
            Err(_) => {
                eprintln!("could not open file {}", entrypath.to_str().unwrap().red());
                continue;
            }
        };
        dotfiles.push(dotfile);
    }
    dotfiles
}

pub fn apply_config_dir(path: &PathBuf) -> bool {
    if !path.is_dir() {
        return false;
    }

    let mut donesomething = false;
    for entry in walk_config_dir(path) {
        let tmpsource = match DotFile::from_pathbuf(&entry.path().to_path_buf()) {
            Ok(file) => file,
            Err(_) => {
                eprintln!(
                    "could not open file {}",
                    &entry.path().to_str().unwrap().red()
                );
                continue;
            }
        };
        if let ApplyResult::Changed = tmpsource.apply() {
            donesomething = true;
        }
    }

    donesomething
}
