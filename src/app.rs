use clap::{arg, command, value_parser, Arg, ArgAction, ArgMatches, ColorChoice, Command};
use std::path::PathBuf;

pub fn get_vec_args<'a>(matches: &'a ArgMatches, name: &str) -> Vec<&'a str> {
    let sections = matches
        .get_many::<String>(name)
        .unwrap_or_default()
        .map(|v| v.as_str())
        .collect::<Vec<_>>();
    return sections;
}

pub fn build_app() -> Command {
    command!()
        .color(ColorChoice::Always)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .about("instant manager of sections in dotfiles")
        .author("paperbenni <paperbenni@gmail.com>")
        .subcommand(
            Command::new("test")
                .about("testing stuff")
                .arg(arg!(-l --list "list test values").action(ArgAction::SetTrue)),
        )
        .subcommand(
            Command::new("compile")
                .about("compile file")
                .arg(
                    Arg::new("file")
                        .value_parser(value_parser!(PathBuf))
                        .required(true)
                        .help("file to compile"),
                )
                .arg(
                    arg!(-m --metafile "use meta file")
                        .required(false)
                        .action(ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("update")
                .about("update sections from sources")
                .arg(
                    arg!(-f --file "file to update")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-p --print "only print result, do not write to file")
                        .required(false)
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    arg!(-s --section "only update section, default is all")
                        .required(false)
                        .action(ArgAction::Append),
                ),
        )
        .subcommand(
            Command::new("query")
                .about("print section from file")
                .arg(
                    arg!(--file "file to search through")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(--section "section to print")
                        .required(true)
                        .action(ArgAction::Append)
                        .value_parser(value_parser!(String)),
                ),
        )
        .subcommand(
            Command::new("info")
                .about("list imosid metadate in file")
                .arg(
                    Arg::new("file")
                        .required(true)
                        .help("file to get info for")
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("apply")
                .about("apply source to target marked in the file")
                .arg(
                    Arg::new("file")
                        .help("file or directory to apply")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .about("force apply even if there are conflicts")
                        .required(false)
                        .takes_value(false),
                ),
        )
        .subcommand(
            Command::new("delete")
                .about("delete section from file")
                .arg(
                    Arg::new("file")
                        .required(true)
                        .help("file to delete section from")
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-p --print "only print result, do not write to file")
                        .required(false)
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    arg!(-s --section "section to delete")
                        .required(true)
                        .action(ArgAction::Append)
                        .value_parser(value_parser!(String))
                        .action(ArgAction::Append),
                ),
        )
        .subcommand(
            Command::new("check")
                .about("check directory for modified files")
                .arg(
                    arg!(--directory "directory to check")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
}
