use clap::{App, AppSettings, Arg};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::{self, prelude::*};
use std::os::unix::prelude::FileExt;
use std::path::Path;
use std::vec;

enum CommentType {
    SectionBegin,
    SectionEnd,
    HashInfo,
}

pub struct Specialcomment {
    line: u32,
    string: String,
    ctype: CommentType,
    argument: String,
}

impl Specialcomment {
    fn new(line: &str, commentsymbol: &str, linenumber: u32) -> Option<Specialcomment> {
        let mut iscomment = String::from("^ *");
        iscomment.push_str(&commentsymbol);
        iscomment.push_str(" *... *([^ ]*) +([^ ]*).*");

        let commentregex = Regex::new(&iscomment).unwrap();
        let keywords = commentregex.captures(&line);
        match keywords {
            Some(captures) => {
                let keyword = captures.get(1).unwrap().as_str();
                println!("keyword {}", keyword);
                let cargument: String;
                let tmptype: CommentType;
                match keyword {
                    "begin" => {
                        if captures.len() >= 3 {
                            cargument = String::from(captures.get(2).unwrap().as_str());
                            tmptype = CommentType::SectionBegin;
                        } else {
                            println!("warning: missing section name on line {}", linenumber);
                            return Option::None;
                        }
                    }
                    "end" => {
                        if captures.len() >= 3 {
                            cargument = String::from(captures.get(2).unwrap().as_str());
                            tmptype = CommentType::SectionEnd;
                        } else {
                            println!("warning: missing section name on line {}", linenumber);
                            return Option::None;
                        }
                    }
                    "hash" => {
                        if captures.len() >= 3 {
                            cargument = String::from(captures.get(2).unwrap().as_str());
                            tmptype = CommentType::HashInfo;
                        } else {
                            println!("warning: missing hash on line {}", linenumber);
                            return Option::None;
                        }
                    }
                    &_ => {
                        println!("warning: incomplete imosid comment on {}", linenumber);
                        return Option::None;
                    }
                }
                Option::Some(Specialcomment {
                    line: linenumber,
                    string: String::from(line),
                    ctype: tmptype,
                    argument: cargument,
                })
            }
            None => {
                return Option::None;
            }
        }
    }
}

pub struct Section {
    startline: u32,
    name: String,
    source: Option<String>,
    endline: u32,
    hash: String,
    content: String,
    broken: bool,
}

impl Section {
    fn new(start: u32, name: String, source: Option<String>, end: u32) -> Section {
        Section {
            name: String::from(name),
            startline: start,
            endline: end,
            source: source,
            hash: String::from(""), //todo
            broken: false,
            content: String::from("asd"),
        }
    }
}

pub struct Specialfile {
    content: String,
    specialcomments: Vec<Specialcomment>,
    sections: Vec<Section>,
    file: File,
}

impl Specialfile {
    fn new(filename: String) -> Specialfile {
        let sourcefile = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filename)
            .unwrap();

        let mut commentvector = Vec::new();
        let mut sectionvector = Vec::new();
        let mut counter = 0;
        for i in io::BufReader::new(&sourcefile).lines() {
            counter += 1;
            let line = i.unwrap();
            let newcomment = Specialcomment::new(&line, "#", counter);
            match newcomment {
                Some(comment) => {
                    commentvector.push(comment);
                }
                None => {}
            }
        }

        let currentname: String;
        let mut sectionstack = Vec::new();
        if commentvector.len() >= 2 {
            for i in commentvector.iter() {
                match i.ctype {
                    CommentType::SectionBegin => {
                        println!("starting section");
                        sectionstack.push(i);
                    }
                    CommentType::SectionEnd => {
                        println!("ending section");
                        match sectionstack.pop() {
                            Some(comment) => {
                                if comment.argument == i.argument {
                                    // closing section
                                    let newsection = Section::new(
                                        comment.line,
                                        String::from(&comment.argument),
                                        Option::None,
                                        i.line,
                                    );
                                    sectionvector.push(newsection);
                                }
                            }
                            None => {}
                        }
                    }
                    CommentType::HashInfo => {
                        println!("hash ingo");
                    }
                }
            }
        }

        let retfile = Specialfile {
            content: String::new(),
            specialcomments: commentvector,
            sections: sectionvector,
            file: sourcefile,
        };
        return retfile;
    }
}

fn main() {
    let inputarg = Arg::new("input")
        .multiple_occurrences(true)
        .short('i')
        .long("input")
        .takes_value(true)
        .required(false)
        .about("add file to source list");

    let matches = App::new("imosid")
        .version("0.1")
        .author("paperbenni <paperbenni@gmail.com>")
        .about("instant manager of sections in dotfiles")
        .arg(Arg::new("syntax").required(false).about("manually set the comment syntax"))
        .subcommand(
            App::new("update")
                .about("apply source sections to target")
                .arg(
                    inputarg
                ).arg(
                    Arg::new("target")
                        .index(1)
                        .required(true)
                        .about("file to apply updates to")
                ).arg(
                    Arg::new("section").long("section")
                        .about("only update section <section>. all sections are included if unspecified")
                        .multiple_occurrences(true).takes_value(true).required(false)
                ).setting(AppSettings::ColoredHelp),
        ).subcommand(
            App::new("compile")
                .about("add hashes to sections in source file")
                .setting(AppSettings::ColoredHelp)
                .arg(
                    Arg::new("file")
                        .index(1)
                        .required(true)
                        .about("file to process")
                )
        ).subcommand(
            App::new("query")
                .about("print section from file")
                .arg(
                    Arg::new("file")
                        .index(1)
                        .about("file to search through")
                        .required(true)
                ).arg(Arg::new("section").index(2).required(false).short('s')),
        )
        .setting(AppSettings::ColoredHelp)
        .get_matches();

    if matches.is_present("compile") {
        if let Some(ref matches) = matches.subcommand_matches("compile") {
            let filename = matches.value_of("file").unwrap();
            println!("{}", filename);

            if Path::new(filename).is_file() {
                let mut linevec = Vec::new();
                let mut contentstring = String::new();
                let queryfile = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(filename)
                    .unwrap();
                let lines = io::BufReader::new(queryfile).lines();
                for i in lines {
                    let line = i.unwrap();
                    linevec.push(line.clone());
                    contentstring.push_str(&line);
                    contentstring.push('\n');
                }
                let mut hasher = Sha256::new();
                hasher.update(contentstring);
                let hasher = hasher.finalize();
                println!("{:X}", hasher);
            }
        }
    }
    let tester = Specialcomment::new("# ... begin stuff", "#", 12).unwrap();
    println!("argument {}", tester.argument);
}
