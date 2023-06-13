mod app;
mod dotwalker;
mod test;
use colored::Colorize;
use dotwalker::{apply_config_dir, walk_config_dir, walk_dotfiles};
mod comment;
mod commentmap;
mod contentline;
mod files;
mod hashable;
mod metafile;
mod section;
use std::{path::PathBuf, println};

use crate::{
    app::get_vec_args,
    files::{ApplyResult, DotFile},
    hashable::Hashable,
    metafile::MetaFile,
    section::Section,
};

pub mod built_info {
    // The file has been placed there by the build script.
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

// clap value parser does not distinguish between files and directories
macro_rules! check_file_arg {
    ($a:expr) => {
        if !$a.is_file() {
            eprintln!("{}", "file does not exist".red().bold());
            //TODO make this an error
            return Ok(());
        }
    };
}

macro_rules! get_dotfile {
    ($a:expr) => {
        match DotFile::from_pathbuf($a) {
            Ok(file) => file,
            Err(_) => {
                eprintln!("could not open file {}", $a.to_str().unwrap().red());
                return Ok(());
            }
        }
    };
}

fn main() -> Result<(), std::io::Error> {
    let imosidapp = app::build_app();
    let matches = imosidapp.get_matches();

    match matches.subcommand() {
        // compile a file, making it an unmodified imosid file
        Some(("compile", compile_matches)) => {
            let filename = compile_matches.get_one::<PathBuf>("file").unwrap();
            check_file_arg!(filename);
            if *compile_matches.get_one("metafile").unwrap() {
                let mut newmetafile = MetaFile::from(filename.to_path_buf());
                newmetafile.compile();
                newmetafile.write_to_file();
                println!("compiled {}", &filename.to_str().unwrap().bold());
                return Ok(());
            }
            let mut compfile = get_dotfile!(filename);
            if compfile.compile() {
                compfile.write_to_file();
                println!("compiled {}", filename.to_str().unwrap().bold());
            } else {
                println!(
                    "{} already compiled, no change",
                    filename.to_str().unwrap().bold().green()
                );
            }
        }
        Some(("check", check_matches)) => {
            let filename = check_matches.get_one::<PathBuf>("directory").unwrap();
            if !filename.is_dir() {
                eprintln!(
                    "{} is not a directory, only directories can be checked",
                    filename.to_str().unwrap().red()
                );
                return Ok(());
            }
            let mut anymodified = false;
            for dotfile in walk_dotfiles(filename) {
                if dotfile.modified {
                    println!("{} {}", dotfile.filename.red().bold(), "modified".red());
                    anymodified = true;
                }
                if !dotfile.is_managed() {
                    println!(
                        "{} {}",
                        dotfile.filename.yellow().bold(),
                        "is unmanaged".yellow()
                    )
                }
            }
        }

        Some(("query", query_matches)) => {
            let filename = query_matches.get_one::<PathBuf>("file").unwrap();
            let query_sections = get_vec_args(query_matches, "section");

            check_file_arg!(filename);

            let queryfile = get_dotfile!(filename);

            if queryfile.metafile.is_some() {
                todo!("add message for this");
                return Ok(());
            }

            for i in &queryfile.sections {
                if let Section::Named(_, named_data) = i {
                    for query in &query_sections {
                        if query.eq(&named_data.name) {
                            println!("{}", i.output(&queryfile.commentsign));
                        }
                    }
                }
            }
        }

        Some(("update", update_matches)) => {
            let filename = update_matches.get_one::<PathBuf>("file").unwrap();

            let sections = get_vec_args(update_matches, "section");

            check_file_arg!(filename);

            let mut updatefile = get_dotfile!(filename);
            updatefile.update();

            match updatefile.metafile {
                Some(_) => {
                    eprintln!("cannot update metafile");
                    return Ok(());
                }
                None => {}
            }

            if sections.is_empty() {
                // update all sections
            }
        }
        Some(("delete", delete_matches)) => {
            let filename = delete_matches.get_one::<PathBuf>("file").unwrap();

            let sections = get_vec_args(delete_matches, "section");

            check_file_arg!(filename);

            let mut deletefile = get_dotfile!(filename);

            for i in sections {
                if deletefile.deletesection(i) {
                    println!("deleted section {}", i.bold());
                } else {
                    println!("could not find section {}", i.red());
                }
            }
            deletefile.write_to_file();
        }

        Some(("apply", apply_matches)) => {
            let filename = apply_matches.get_one::<PathBuf>("file").unwrap();
            if filename.is_dir() {
                if !apply_config_dir(filename) {
                    println!("{}", "nothing to do".bold());
                }
                return Ok(());
            } else if filename.is_file() {
                let tmpsource = get_dotfile!(filename);
                tmpsource.apply();
            } else {
                eprintln!("{}", "file does not exist".red().bold());
                return Ok(());
            }
        }
        Some(("info", info_matches)) => {
            let filename = info_matches.get_one::<PathBuf>("file").unwrap();
            check_file_arg!(filename);
            let infofile = DotFile::from_pathbuf(filename)?;
            println!("{}", infofile.pretty_info());

            if infofile.modified {
                // give caller an easy way to tell if a file is modified
                std::process::exit(1);
            }
        }
        Some((&_, _)) => {
            //TODO: do this better
            return Ok(());
        }
        None => {
            return Ok(());
        }
    }
    return Ok(());
}
