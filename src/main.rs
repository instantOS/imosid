use clap::{App, AppSettings, Arg, ArgMatches};
use dirs::home_dir;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::{self, prelude::*};
use std::ops::Deref;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CommentType {
    SectionBegin,
    SectionEnd,
    SourceInfo,
    TargetInfo,
    HashInfo,
}

#[derive(Clone)]
pub struct Specialcomment {
    line: u32,
    content: String,
    section: String,
    ctype: CommentType,
    argument: Option<String>,
}

pub struct ContentLine {
    linenumber: u32,
    content: String,
}

impl Specialcomment {
    fn new(line: &str, commentsymbol: &str, linenumber: u32) -> Option<Specialcomment> {
        let mut iscomment = String::from("^ *");
        iscomment.push_str(&commentsymbol);
        iscomment.push_str(" *... *(.*)");

        let commentregex = Regex::new(&iscomment).unwrap();

        let keywords = commentregex.captures(&line);
        match &keywords {
            Some(captures) => {
                let keywords = captures
                    .get(1)
                    .unwrap()
                    .as_str()
                    .split(" ")
                    .collect::<Vec<&str>>();

                // needs at least a section and a keyword
                if keywords.len() < 2 {
                    return Option::None;
                }

                let sectionname = keywords[0];
                let keyword = keywords[1];
                let cargument: Option<String>;

                if keywords.len() > 2 {
                    cargument = Option::Some(String::from(keywords[2]));
                } else {
                    cargument = Option::None;
                }

                let tmptype: CommentType;
                match keyword {
                    "begin" => {
                        tmptype = CommentType::SectionBegin;
                    }
                    "end" => {
                        tmptype = CommentType::SectionEnd;
                    }
                    "hash" => {
                        tmptype = CommentType::HashInfo;
                        match cargument {
                            Some(_) => {}
                            None => {
                                println!("missing hash value on line {}", linenumber);
                                return Option::None;
                            }
                        }
                    }
                    "source" => {
                        tmptype = CommentType::SourceInfo;
                        match cargument {
                            Some(_) => {}
                            None => {
                                println!("missing source file on line {}", linenumber);
                                return Option::None;
                            }
                        }
                    }

                    "target" => {
                        if sectionname == "all" {
                            tmptype = CommentType::TargetInfo;
                            match cargument {
                                Some(_) => {}
                                None => {
                                    println!("missing target value on line {}", linenumber);
                                    return Option::None;
                                }
                            }
                        } else {
                            println!(
                                "warning: target can only apply to the whole file {}",
                                linenumber
                            );
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
                    content: String::from(line),
                    section: String::from(sectionname),
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

#[derive(Clone)]
pub struct Section {
    startline: u32,
    name: Option<String>,
    source: Option<String>,
    endline: u32,
    hash: String,
    targethash: Option<String>,
    content: String,
    modified: bool,
}

impl Section {
    fn new(
        start: u32,
        end: u32,
        name: Option<String>,
        source: Option<String>,
        targethash: Option<String>,
    ) -> Section {
        Section {
            name: name,
            startline: start,
            endline: end,
            source: source,
            hash: match &targethash {
                Some(hash) => String::from(hash),
                None => String::new(),
            },
            targethash: targethash,
            modified: false,
            content: String::from(""),
        }
    }

    fn compile(&mut self) {
        self.targethash = Option::Some(self.hash.clone());
    }

    fn is_anonymous(&self) -> bool {
        match &self.name {
            Some(_) => false,
            None => true,
        }
    }

    fn finalize(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(&self.content);
        let hasher = hasher.finalize();
        let newhash = format!("{:X}", hasher);
        match &self.name {
            Some(_) => {
                if self.hash == newhash {
                    self.modified = false;
                } else {
                    self.modified = true;
                }
            }
            // anonymous section
            None => {
                self.hash = newhash;
            }
        }
        self.hash = String::from(format!("{:X}", hasher));
    }

    fn push_str(&mut self, line: &str) {
        self.content.push_str(line);
        self.content.push('\n');
    }

    fn output(&self, commentsign: &str) -> String {
        let mut outstr = String::new();
        match &self.name {
            Some(name) => {
                outstr.push_str(&format!("{}... {} begin\n", commentsign, name));
                outstr.push_str(&format!(
                    "{}... {} hash {}\n",
                    commentsign,
                    name,
                    if self.targethash.is_some() {
                        self.targethash.clone().unwrap()
                    } else {
                        self.hash.clone()
                    }
                ));
                match &self.source {
                    Some(source) => {
                        outstr.push_str(&format!("{}... {} begin\n", commentsign, source));
                    }
                    None => {}
                } //todo: section target
                outstr.push_str(&self.content);
                outstr.push_str(&format!("{}... {} end\n", commentsign, name));
            }
            // anonymous section
            None => {
                outstr = self.content.clone();
                return outstr;
            }
        }
        return outstr;
    }
}

pub struct Specialfile {
    specialcomments: Vec<Specialcomment>,
    sections: Vec<Section>,
    file: File,
    filename: String,
    targetfile: Option<String>,
    commentsign: String,
}

impl Specialfile {
    fn new(filename: &str) -> Specialfile {
        let sourcepath = Path::new(filename)
            .canonicalize()
            .unwrap()
            .display()
            .to_string();

        let sourcefile = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filename)
            .unwrap();

        let mut commentvector = Vec::new();
        let mut counter = 0;

        let mut sectionvector: Vec<Section> = Vec::new();
        let mut contentvector: Vec<ContentLine> = Vec::new();

        let mut sectionmap: HashMap<String, Vec<Specialcomment>> = HashMap::new();

        let filelines = io::BufReader::new(&sourcefile).lines();

        let mut targetfile: Option<String> = Option::None;
        let mut commentsign = String::new();
        let mut hascommentsign = false;

        for i in filelines {
            counter += 1;
            let line = i.unwrap();
            if !hascommentsign {
                commentsign = String::from(get_comment_sign(&sourcepath, &line));
                hascommentsign = true;
            }
            let newcomment = Specialcomment::new(&line, &commentsign, counter);
            match newcomment {
                Some(comment) => {
                    // comments with section all apply to the entire file
                    if &comment.section == "all" {
                        match &comment.ctype {
                            CommentType::TargetInfo => {
                                if comment.argument.is_some() {
                                    targetfile =
                                        Option::Some(String::from(&comment.argument.unwrap()));
                                }
                            }
                            &_ => {}
                        }
                        continue;
                    }
                    commentvector.push(comment.clone());
                    if sectionmap.contains_key(&comment.section) {
                        sectionmap.get_mut(&comment.section).unwrap().push(comment);
                    } else {
                        let mut sectionvector = Vec::new();
                        sectionvector.push(comment.clone());
                        sectionmap.insert(comment.section, sectionvector);
                    }
                }
                None => contentvector.push(ContentLine {
                    linenumber: counter,
                    content: line,
                }),
            }
        }

        for (sectionname, svector) in sectionmap.iter() {
            let mut checkmap = HashMap::new();
            for i in svector.iter() {
                if checkmap.contains_key(&i.ctype) {
                    break;
                } else {
                    checkmap.insert(&i.ctype, i);
                }
            }
            if !(checkmap.contains_key(&CommentType::SectionBegin)
                && checkmap.contains_key(&CommentType::SectionEnd)
                && checkmap.contains_key(&CommentType::HashInfo))
            {
                println!("warning: invalid section {}", sectionname);
                continue;
            }

            let newsection = Section::new(
                checkmap.get(&CommentType::SectionBegin).unwrap().line,
                checkmap.get(&CommentType::SectionEnd).unwrap().line,
                Option::Some(String::from(sectionname)),
                Option::None, //source todo
                Option::Some(
                    checkmap
                        .get(&CommentType::HashInfo)
                        .unwrap()
                        .argument
                        .clone()
                        .unwrap()
                        .clone(),
                ),
            );

            sectionvector.push(newsection);
        }

        sectionvector.sort_by(|a, b| a.startline.cmp(&b.startline));

        // detect overlapping sections
        let vecsize = sectionvector.len();
        let mut broken_indices = Vec::new();
        let mut skipnext = false;
        for i in 0..vecsize {
            if skipnext {
                skipnext = false;
                continue;
            }
            let currentsection = &sectionvector[i];
            if i < vecsize - 1 {
                let nextsection = &sectionvector[i + 1];
                if nextsection.startline < currentsection.endline {
                    broken_indices.push(i + 1);
                    broken_indices.push(i);
                    skipnext = true;
                }
            }
        }

        for i in broken_indices {
            println!("section {} overlapping", i);
            sectionvector.remove(i);
        }

        let mut currentline = 1;
        let mut tmpstart;
        let mut tmpend;
        let mut anonvector: Vec<Section> = Vec::new();
        for i in &sectionvector {
            if i.startline - currentline >= 1 {
                tmpstart = currentline;
                tmpend = i.startline - 1;
                let newsection =
                    Section::new(tmpstart, tmpend, Option::None, Option::None, Option::None);
                anonvector.push(newsection);
            }
            currentline = i.endline + 1;
        }

        sectionvector.extend(anonvector);
        sectionvector.sort_by(|a, b| a.startline.cmp(&b.startline));

        for i in &mut sectionvector {
            // TODO: speed this up, binary search or something
            for c in &contentvector {
                if c.linenumber > i.endline {
                    break;
                } else if c.linenumber < i.startline {
                    continue;
                }
                i.push_str(&c.content);
            }
            i.finalize();
        }

        let retfile = Specialfile {
            specialcomments: commentvector,
            sections: sectionvector,
            file: sourcefile,
            filename: sourcepath,
            targetfile: targetfile,
            commentsign: commentsign,
        };

        return retfile;
    }

    fn compile(&mut self) {
        for i in 0..self.sections.len() {
            self.sections[i].compile();
        }
    }

    fn write_to_file(&self) {
        let newfile = File::create(&expand_tilde(&self.filename));
        match newfile {
            Err(_) => {
                println!("error: could not write to file {}", &self.filename);
                panic!("write_to_file");
            }
            Ok(mut file) => {
                file.write_all(self.output().as_bytes()).unwrap();
            }
        }
    }

    fn applyfile(&mut self, inputfile: &Specialfile) -> bool {
        let mut modified = false;

        for i in &inputfile.sections {
            if self.applysection(i.clone()) {
                modified = true;
            }
        }
        return modified;
    }

    fn output(&self) -> String {
        let mut retstr = String::new();
        let mut firstsection: Option<String> = Option::None;

        // respect hashbang
        if self.targetfile.is_some() {
            if self.sections.get(0).unwrap().is_anonymous() {
                let firstline = String::from(
                    self.sections
                        .get(0)
                        .unwrap()
                        .content
                        .split("\n")
                        .nth(0)
                        .unwrap(),
                );
                let originalcontent = &self.sections.get(0).unwrap().content;

                if Regex::new("^#!/.*").unwrap().is_match(&firstline) {
                    let mut newcontent = String::from(&firstline);
                    newcontent.push_str(&format!(
                        "\n{}... all target {}\n",
                        &self.commentsign,
                        &(self.targetfile.clone().unwrap())
                    ));
                    // reappend original section content
                    newcontent.push_str(originalcontent.trim_start_matches(&firstline));
                    firstsection = Option::Some(newcontent);
                } else {
                    let mut newcontent = String::from(format!(
                        "{}...all target {}\n",
                        self.commentsign,
                        self.targetfile.clone().unwrap()
                    ));
                    newcontent.push_str(originalcontent);
                    firstsection = Option::Some(newcontent);
                }
            } else {
                let mut newcontent = String::from(&self.commentsign);
                newcontent.push_str("... all target");
                newcontent.push_str(&(self.targetfile.clone().unwrap()));
                newcontent.push('\n');

                newcontent.push_str(&self.sections.get(0).unwrap().content);
                firstsection = Option::Some(newcontent);
            }
        }

        for i in &self.sections {
            if firstsection.is_some() {
                retstr.push_str(&firstsection.unwrap());
                firstsection = Option::None;
            } else {
                retstr.push_str(&i.output(&self.commentsign));
            }
        }
        return retstr;
    }

    fn applysection(&mut self, section: Section) -> bool {
        for i in 0..self.sections.len() {
            let tmpsection = self.sections.get(i).unwrap();
            if tmpsection.is_anonymous()
                || section.is_anonymous()
                || section.modified
                || tmpsection.modified
            {
                continue;
            }
            let tmpname = &tmpsection.name.clone().unwrap();
            if tmpname == &section.name.clone().unwrap() {
                if &tmpsection.hash == &section.hash {
                    continue;
                }
                self.sections[i] = section;
                return true;
            }
        }
        return false;
    }
}

fn expand_tilde(input: &str) -> String {
    let mut retstr = String::from(input);
    if retstr.starts_with("~/") {
        retstr = String::from(format!(
            "{}/{}",
            home_dir().unwrap().into_os_string().into_string().unwrap(),
            retstr.strip_prefix("~/").unwrap()
        ));
    }
    return retstr;
}

fn get_comment_sign(filename: &str, firstline: &str) -> String {
    let fpath = Path::new(filename);

    let mut file_name_commentsigns: HashMap<&str, &str> = HashMap::new();
    file_name_commentsigns.insert("dunstrc", "#");
    file_name_commentsigns.insert("zshrc", "#");
    file_name_commentsigns.insert("bashrc", "#");
    file_name_commentsigns.insert("Xresources", "!");

    // get comment syntax via file name
    let fname = fpath.file_name().and_then(OsStr::to_str);
    match fname {
        Some(name) => {
            let filename = String::from(String::from(name).trim_start_matches("."));
            match file_name_commentsigns.get(filename.as_str()) {
                Some(sign) => {
                    return String::from(sign.deref());
                }
                None => {}
            }
        }
        None => {}
    }

    let mut file_type_commentsigns: HashMap<&str, &str> = HashMap::new();
    file_type_commentsigns.insert("py", "#");
    file_type_commentsigns.insert("sh", "#");
    file_type_commentsigns.insert("zsh", "#");
    file_type_commentsigns.insert("bash", "#");
    file_type_commentsigns.insert("fish", "#");
    file_type_commentsigns.insert("c", "//");
    file_type_commentsigns.insert("cpp", "//");
    file_type_commentsigns.insert("rasi", "//");
    file_type_commentsigns.insert("desktop", "#");
    file_type_commentsigns.insert("conf", "#");
    file_type_commentsigns.insert("vim", "\"");
    file_type_commentsigns.insert("reg", ";");
    file_type_commentsigns.insert("ini", ";");

    let ext = fpath.extension().and_then(OsStr::to_str);

    // get comment syntax via file extension
    match ext {
        Some(extension) => {
            let tester = file_type_commentsigns.get(extension);
            match tester {
                Some(sign) => {
                    return String::from(sign.deref());
                }
                None => {}
            }
        }
        None => {}
    }

    // get comment syntax via #!/hashbang

    let mut file_hashbang_commentsigns: HashMap<&str, &str> = HashMap::new();

    file_hashbang_commentsigns.insert("python", "#");
    file_hashbang_commentsigns.insert("sh", "#");
    file_hashbang_commentsigns.insert("bash", "#");
    file_hashbang_commentsigns.insert("zsh", "#");
    file_hashbang_commentsigns.insert("fish", "#");
    file_hashbang_commentsigns.insert("node", "//");

    match Regex::new("^#!/.*[/ ](.*)$").unwrap().captures(&firstline) {
        Some(captures) => {
            let application = captures.get(1).unwrap().as_str();
            match file_hashbang_commentsigns.get(application) {
                Some(sign) => {
                    return String::from(sign.deref());
                }
                None => {}
            }
        }
        None => {}
    }

    return String::from("#");
}

fn get_special_file(matches: &ArgMatches, name: &str) -> Option<Specialfile> {
    let filename = matches.value_of(name).unwrap();
    if Path::new(filename).is_file() {
        let retfile = Specialfile::new(filename);
        return Some(retfile);
    }
    None
}

fn main() -> Result<(), std::io::Error> {
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
                )
                .arg(Arg::new("print")
                        .short('p')
                        .long("print")
                        .required(false)
                        .about("only print results, do not write to file")
                        .takes_value(false))
                .arg(
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
                ).arg(
                    Arg::new("section").
                        required(false).short('s').
                        long("section").
                        multiple_occurrences(true).takes_value(true)
                    ),
        ).subcommand(
            App::new("info").about("list imosid metadata in file").arg(
                Arg::new("file").index(1).required(true).about("file to get info for")
            )
        ).subcommand(
            App::new("apply").about("apply source to target marked in the file").arg(
                Arg::new("file").index(1).required(true).about("file to apply")
            )
        )
        .setting(AppSettings::ColoredHelp).setting(AppSettings::ArgRequiredElseHelp)
        .get_matches();

    if matches.is_present("compile") {
        if let Some(ref matches) = matches.subcommand_matches("compile") {
            let filename = matches.value_of("file").unwrap();
            if Path::new(filename).is_file() {
                let mut testfile = Specialfile::new(filename);
                testfile.compile();
                testfile.write_to_file();
            }
        }
    }

    if matches.is_present("info") {
        // todo: colored output
        if let Some(ref matches) = matches.subcommand_matches("info") {
            let filename = matches.value_of("file").unwrap();
            if Path::new(filename).is_file() {
                let infofile = Specialfile::new(filename);
                let commentsign = &infofile.commentsign;
                println!("comment syntax: {}", commentsign);
                match infofile.targetfile {
                    Some(target) => {
                        println!("target: {}", &target);
                    }
                    None => {}
                }
                for i in infofile.sections {
                    if !i.name.is_some() {
                        continue;
                    }
                    if i.modified {
                        println!(
                            "{}-{}: {} | modified",
                            i.startline,
                            i.endline,
                            i.name.unwrap()
                        );
                    } else {
                        println!("{}-{}: {} | Ok", i.startline, i.endline, i.name.unwrap());
                    }
                }
            } else {
                println!("file {} not found", filename);
            }
        }
    }

    if matches.is_present("apply") {
        // todo: create file with folder and all sections if not existing
        if let Some(ref matches) = matches.subcommand_matches("apply") {
            let sourcefile = get_special_file(&matches, "file");
            if !sourcefile.is_some() {
                // todo: error message
                return Ok(());
            }
            let sourcefile = sourcefile.unwrap();
            match &sourcefile.targetfile {
                None => {
                    println!("No target comment found in {}", &sourcefile.filename);
                    return Ok(());
                }
                Some(targetname) => {
                    let realtargetname = expand_tilde(targetname);

                    let checkpath = Path::new(&realtargetname);
                    if !checkpath.is_file() {
                        let bufpath = checkpath.to_path_buf();
                        match bufpath.parent() {
                            Some(parent) => {
                                std::fs::create_dir_all(parent.to_str().unwrap()).unwrap();
                            }
                            None => {}
                        }
                        File::create(&realtargetname)?;
                        let targetfile: Specialfile = Specialfile {
                            specialcomments: sourcefile.specialcomments,
                            sections: sourcefile.sections,
                            file: sourcefile.file,
                            filename: targetname.clone(),
                            targetfile: Option::Some(targetname.clone()),
                            commentsign: sourcefile.commentsign,
                        };
                        targetfile.write_to_file();
                    } else {
                        let mut targetfile = Specialfile::new(&realtargetname);
                        if targetfile.applyfile(&sourcefile) {
                            targetfile.write_to_file();
                        }
                    }
                }
            }
        }
    }
    if matches.is_present("update") {
        if let Some(ref matches) = matches.subcommand_matches("update") {
            if matches.value_of("target").unwrap() == matches.value_of("input").unwrap() {
                return Ok(());
            }

            let mut targetfile = get_special_file(matches, "target").unwrap();
            let inputfile = get_special_file(matches, "input").unwrap();

            let mut modified = false;

            for i in inputfile.sections {
                if targetfile.applysection(i) {
                    modified = true;
                }
            }

            if matches.is_present("print") {
                println!("{}", targetfile.output());
            } else {
                if modified {
                    let mut newfile = File::create(&targetfile.filename)?;
                    newfile.write_all(targetfile.output().as_bytes())?;
                    println!("updated file");
                } else {
                    println!("no updates necessary");
                }
            }
        }
    }

    if matches.is_present("query") {
        if let Some(ref matches) = matches.subcommand_matches("query") {
            let filename = matches.value_of("file").unwrap();

            if Path::new(filename).is_file() {
                let testfile = Specialfile::new(filename);

                match matches.values_of("section") {
                    Some(sections) => {
                        let sections = sections.into_iter();
                        for i in sections {
                            for s in &testfile.sections {
                                if let Some(name) = &s.name {
                                    if name == i {
                                        println!("{}", s.output(&testfile.commentsign));
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        println!("{}", testfile.output());
                    }
                }
            } else {
                eprintln!("file {} not found", filename);
            }
        }
    }
    Ok(())
}
