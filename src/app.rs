use clap::{App, AppSettings, Arg};

pub fn build_app() -> App<'static> {
    // Use of a mod or pub mod is not actually necessary.
pub mod built_info {
   // The file has been placed there by the build script.
   include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

    let inputarg = Arg::new("input")
        .multiple_occurrences(true)
        .short('i')
        .long("input")
        .takes_value(true)
        .required(false)
        .help("add file to source list");

    let metafilearg = Arg::new("metafile")
        .required(false)
        .short('m')
        .long("metafile")
        .takes_value(false)
        .help("put imosid metadata into a separate file instead of using comments in the file");

    let app = App::new("imosid")
        .version(built_info::PKG_VERSION)
        .author("paperbenni <paperbenni@gmail.com>")
        .about("instant manager of sections in dotfiles")
        .arg(Arg::new("syntax").required(false).help("manually set the comment syntax"))
        .subcommand(
            App::new("update")
                .help("apply source sections to target")
                .arg(
                    inputarg
                ).arg(
                    Arg::new("target")
                        .index(1)
                        .required(true)
                        .help("file to apply updates to")
                )
                .arg(Arg::new("print")
                        .short('p')
                        .long("print")
                        .required(false)
                        .help("only print results, do not write to file")
                        .takes_value(false))
                .arg(
                    Arg::new("section").long("section")
                        .help("only update section <section>. all sections are included if unspecified")
                        .multiple_occurrences(true).takes_value(true).required(false)
                ).setting(AppSettings::ColoredHelp),
        ).subcommand(
            App::new("compile")
                .about("add hashes to sections in source file")
                .setting(AppSettings::ColoredHelp).arg(&metafilearg)
                .arg(
                    Arg::new("file")
                        .index(1)
                        .required(true)
                        .help("file to process")
                )
        ).subcommand(
            App::new("check")
                .about("check folder for modified files")
                .setting(AppSettings::ColoredHelp)
                .arg(
                    Arg::new("directory")
                        .index(1)
                        .required(true)
                        .help("directory to check")
                )
        ).subcommand(
            App::new("query")
                .about("print section from file")
                .arg(
                    Arg::new("file")
                        .index(1)
                        .help("file to search through")
                        .required(true)
                ).arg(
                    Arg::new("section").
                        required(false).short('s').
                        long("section").
                        multiple_occurrences(true).takes_value(true)
                    ),
        ).subcommand(
            App::new("info").about("list imosid metadata in file").arg(
                Arg::new("file").index(1).required(true).help("file to get info for")
            )
        ).subcommand(
            App::new("apply").about("apply source to target marked in the file").arg(
                Arg::new("file").index(1).required(true).help("file to apply")
            )
        )
        .setting(AppSettings::ColoredHelp).setting(AppSettings::ArgRequiredElseHelp);

    return app;
}
