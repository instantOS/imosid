use clap::{crate_version, App, AppSettings, Arg, ArgMatches};
use colored::Colorize;
use dirs::home_dir;
use regex::Regex;
use semver::Version;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{read_to_string, File, OpenOptions};
use std::io::{self, prelude::*, BufRead, ErrorKind};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use toml::Value;

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
        if !line.starts_with(commentsymbol) {
            return Option::None;
        }
        let mut iscomment = String::from("^ *");
        iscomment.push_str(&commentsymbol);
        iscomment.push_str(" *\\.\\.\\. *(.*)");

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
                    "begin" | "start" => {
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

// sections that files get divided into
// these can be independently updated or broken
impl Section {
    fn new(
        start: u32,
        end: u32,
        name: Option<String>,
        source: Option<String>,
        targethash: Option<String>,
    ) -> Section {
        Section {
            name,
            startline: start,
            endline: end,
            source,
            hash: match &targethash {
                Some(hash) => String::from(hash),
                None => String::new(),
            },
            targethash,
            modified: false,
            content: String::from(""),
        }
    }

    // set target hash to current hash
    fn compile(&mut self) {
        self.targethash = Option::Some(self.hash.clone());
    }

    // anonymous sections are sections without marker comments
    // e.g. parts not tracked by imosid
    fn is_anonymous(&self) -> bool {
        match &self.name {
            Some(_) => false,
            None => true,
        }
    }

    // generate section hash
    // and detect section status
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

    // append string to content
    fn push_str(&mut self, line: &str) {
        self.content.push_str(line);
        self.content.push('\n');
    }

    // return entire section with formatted marker comments and content
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

pub struct Metafile {
    hash: String,
    parentfile: String,
    targetfile: Option<String>,
    sourcefile: Option<String>,
    modified: bool,
    imosidversion: Version,
    syntaxversion: i64,
    value: Value,
    content: String,
    path: PathBuf,
}

impl Metafile {
    fn new(path: PathBuf, content: &str) -> Option<Metafile> {
        if !path.is_file() {
            return None;
        }
        let metacontent = read_to_string(&path);
        match metacontent {
            Err(_) => {
                return None;
            }
            Ok(mcontent) => {
                let value = mcontent.parse::<Value>().expect("failed to read toml");

                let mut retfile = Metafile {
                    targetfile: None,
                    sourcefile: None,
                    hash: String::from(""),
                    parentfile: String::from(""),
                    // default version strings
                    imosidversion: Version::new(0, 0, 0),
                    syntaxversion: 1,
                    value: value.clone(),
                    content: String::from(content),
                    modified: false,
                    path,
                };

                // hash and parent are mandatory
                if let Some(Value::String(hash)) = value.get("hash") {
                    retfile.hash = String::from(hash);
                } else {
                    return None;
                }

                if let Some(Value::String(parentfile)) = value.get("parent") {
                    retfile.parentfile = String::from(parentfile);
                } else {
                    return None;
                }

                if let Some(Value::String(targetfile)) = value.get("target") {
                    retfile.targetfile = Some(String::from(targetfile));
                }

                if let Some(Value::String(sourcefile)) = value.get("source") {
                    retfile.sourcefile = Some(String::from(sourcefile));
                }

                if let Some(Value::Integer(syntaxversion)) = value.get("syntaxversion") {
                    retfile.syntaxversion = syntaxversion.clone();
                }

                if let Some(Value::String(imosidversion)) = value.get("imosidversion") {
                    if let Ok(version) = Version::parse(imosidversion) {
                        retfile.imosidversion = version;
                    }
                }

                return Some(retfile);
            }
        };
    }

    // create a new metafile for a file
    fn from(mut path: PathBuf) -> Metafile {
        //TODO handle result
        let filecontent =
            read_to_string(&path).expect("could not read file content to create metafile");

        let parentname = path
            .file_name()
            .unwrap()
            .to_os_string()
            .into_string()
            .unwrap();

        //TODO don't create metafiles to metafiles

        let filename = format!("{}.imosid.toml", parentname);

        path.pop();
        path.push(filename);

        let mut retfile: Metafile;
        if path.is_file() {
            retfile = Metafile::new(path.clone(), &filecontent).expect("could not create metafile");
            retfile.update();
            retfile.finalize();
        } else {
            retfile = Metafile {
                targetfile: None,
                sourcefile: None,
                hash: String::from("placeholder"),
                parentfile: String::from(&parentname),
                imosidversion: Version::parse(crate_version!()).unwrap(),
                syntaxversion: 0,
                value: Value::Integer(0),
                content: String::from(&filecontent),
                modified: false,
                path,
            };
            println!("created new metafile for {}", &parentname);

            retfile.update();
            retfile.compile();
            retfile.write_to_file();
        }

        retfile
    }

    fn get_content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.content);
        let hasher = hasher.finalize();
        format!("{:X}", hasher)
    }

    // check for modifications
    fn finalize(&mut self) {
        self.modified = self.hash != self.get_content_hash();
    }

    fn compile(&mut self) {
        self.hash = self.get_content_hash();
        self.modified = false;
    }

    // populate toml value with data
    fn update(&mut self) {
        let mut selfmap = toml::map::Map::new();
        selfmap.insert(
            String::from("hash"),
            Value::String(String::from(&self.hash)),
        );
        selfmap.insert(
            String::from("parent"),
            Value::String(String::from(&self.parentfile)),
        );
        if let Some(targetfile) = &self.targetfile {
            selfmap.insert(
                String::from("target"),
                Value::String(String::from(targetfile)),
            );
        }
        if let Some(sourcefile) = &self.sourcefile {
            selfmap.insert(
                String::from("source"),
                Value::String(String::from(sourcefile)),
            );
        }

        selfmap.insert(String::from("syntaxversion"), Value::Integer(0));

        selfmap.insert(
            String::from("imosidversion"),
            Value::String(self.imosidversion.to_string()),
        );

        selfmap.insert(
            String::from("syntaxversion"),
            Value::String(self.syntaxversion.to_string()),
        );
        self.value = Value::Table(selfmap);
    }

    fn output(&mut self) -> String {
        self.update();
        self.value.to_string()
    }

    fn write_to_file(&mut self) {
        let newfile = File::create(&self.path);
        match newfile {
            Err(_) => {
                eprintln!("{}", "Error: could not write metafile".red());
            }
            Ok(mut file) => {
                file.write_all(self.output().as_bytes())
                    .expect("could not write metafile");
            }
        }
    }
}

pub struct Specialfile {
    specialcomments: Vec<Specialcomment>,
    sections: Vec<Section>,
    file: File,
    filename: String,
    targetfile: Option<String>,
    metafile: Option<Metafile>,
    commentsign: String,
}

impl Specialfile {
    fn new(filename: &str) -> Result<Specialfile, std::io::Error> {
        let sourcepath = Path::new(filename)
            .canonicalize()
            .expect("could not canonicalize path")
            .display()
            .to_string();

        let sourcefile = OpenOptions::new().read(true).write(true).open(filename)?;
        let metafile;

        let mut commentvector = Vec::new();
        let mut counter = 0;

        let mut sectionvector: Vec<Section> = Vec::new();
        let mut contentvector: Vec<ContentLine> = Vec::new();

        let mut sectionmap: HashMap<String, Vec<Specialcomment>> = HashMap::new();

        let mut targetfile: Option<String> = Option::None;
        let mut commentsign = String::new();
        let mut hascommentsign = false;

        // check for metafile
        if Path::new(&format!("{}.imosid.toml", sourcepath)).is_file() {
            let mut contentstring = String::new();
            io::BufReader::new(&sourcefile).read_to_string(&mut contentstring)?;

            metafile = if let Some(mut metafile) = Metafile::new(
                PathBuf::from(&format!("{}.imosid.toml", sourcepath)),
                &contentstring,
            ) {
                metafile.finalize();
                metafile
            } else {
                return Err(std::io::Error::new(ErrorKind::Other, "invalid metafile"));
            };
            return Ok(Specialfile {
                specialcomments: commentvector,
                sections: sectionvector,
                file: sourcefile,
                filename: sourcepath,
                targetfile: metafile.targetfile.clone(),
                metafile: Some(metafile),
                commentsign: String::from(""),
            });
        } else {
            let filelines = io::BufReader::new(&sourcefile).lines();

            // parse lines for special comments
            for i in filelines {
                counter += 1;
                let line = i?;
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

            // validate sections and initialze section structs
            for (sectionname, svector) in sectionmap.iter() {
                let mut checkmap = HashMap::new();
                // sections cannot have multiple hashes, beginnings etc
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
                    match checkmap.get(&CommentType::SourceInfo) {
                        Some(source) => Some(String::from(source.argument.clone().unwrap())),
                        None => None,
                    },
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

            // sort sections by lines (retaining the original order of the file)
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

            // introduce anonymous sections
            if sectionvector.len() > 0 {
                let mut currentline = 1;
                let mut tmpstart;
                let mut tmpend;
                let mut anonvector: Vec<Section> = Vec::new();
                for i in &sectionvector {
                    if i.startline - currentline >= 1 {
                        tmpstart = currentline;
                        tmpend = i.startline - 1;
                        let newsection = Section::new(
                            tmpstart,
                            tmpend,
                            Option::None,
                            Option::None,
                            Option::None,
                        );
                        anonvector.push(newsection);
                    }
                    currentline = i.endline + 1;
                }

                sectionvector.extend(anonvector);
                sectionvector.sort_by(|a, b| a.startline.cmp(&b.startline));
            } else {
                let newsection = Section::new(
                    1,
                    contentvector.len() as u32,
                    Option::None,
                    Option::None,
                    Option::None,
                );
                sectionvector.push(newsection);
            }

            // fill sections with content
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
                if !i.is_anonymous() {
                    i.finalize();
                }
            }
        }

        let retfile = Specialfile {
            specialcomments: commentvector,
            sections: sectionvector,
            file: sourcefile,
            filename: sourcepath,
            targetfile,
            commentsign,
            metafile: None,
        };

        return Ok(retfile);
    }

    fn get_section(&self, name: &str) -> Option<Section> {
        for i in &self.sections {
            if let Some(sname) = &i.name {
                if sname == name {
                    return Some(i.clone());
                }
            }
        }
        None
    }

    fn compile(&mut self) {
        match &mut self.metafile {
            None => {
                for i in 0..self.sections.len() {
                    self.sections[i].compile();
                }
            }
            Some(metafile) => {
                metafile.compile();
            }
        }
    }

    fn write_to_file(&mut self) {
        let newfile = File::create(&expand_tilde(&self.filename));
        match newfile {
            Err(_) => {
                println!("error: could not write to file {}", &self.filename);
                panic!("write_to_file");
            }
            Ok(mut file) => match &mut self.metafile {
                None => {
                    file.write_all(self.output().as_bytes()).unwrap();
                }
                Some(metafile) => {
                    file.write_all(metafile.content.as_bytes()).unwrap();
                    metafile.write_to_file();
                }
            },
        }
    }

    fn create_file(source: Specialfile) {
        let targetpath = String::from(source.targetfile.clone().unwrap());
        let realtargetpath = expand_tilde(&targetpath);
        // create new file
        match &source.metafile {
            None => {
                let mut targetfile: Specialfile = Specialfile {
                    specialcomments: source.specialcomments,
                    sections: source.sections,
                    filename: realtargetpath.clone(),
                    targetfile: Option::Some(targetpath),
                    commentsign: source.commentsign,
                    file: source.file,
                    metafile: None,
                };
                targetfile.write_to_file();
            }
            Some(metafile) => {
                if metafile.modified {
                    println!("{} modified, skipping", &source.filename);
                    return ();
                }
                OpenOptions::new()
                    .write(true)
                    .open(&realtargetpath)
                    .expect(&format!("cannot open file {}", &targetpath))
                    .write_all(metafile.content.as_bytes())
                    .expect(&format!("could not write file {}", &targetpath));
                let mut newmetafile = Metafile::from(PathBuf::from(&realtargetpath));
                newmetafile.sourcefile = Some(source.filename);
                newmetafile.write_to_file();
            }
        }
    }

    // return true if file will be modified
    fn applyfile(&mut self, inputfile: &Specialfile) -> bool {
        match &mut self.metafile {
            None => {
                if inputfile.metafile.is_some() {
                    eprintln!("cannot apply metafile to normal imosid file");
                    return false;
                }
                //if no sections are updated, don't do anything to the file system
                let mut modified = false;

                for i in &inputfile.sections {
                    if self.applysection(i.clone()) {
                        modified = true;
                    }
                }
                return modified;
            }

            // apply entire content if file is managed by metafile
            Some(metafile) => {
                if !metafile.modified {
                    match &inputfile.metafile {
                        None => {
                            eprintln!(
                                "{}",
                                "cannot apply section file to files managed by metafiles"
                            );
                        }
                        Some(applymetafile) => {
                            if applymetafile.modified {
                                println!("source file {} modified", &applymetafile.parentfile);
                                return false;
                            }
                            metafile.content = applymetafile.content.clone();
                            return true;
                        }
                    }
                } else {
                    println!("{} modified, skipping", &self.filename);
                }
                return false;
            }
        }
    }

    fn output(&self) -> String {
        match &self.metafile {
            None => {
                let mut retstr = String::new();
                let mut firstsection: Option<String> = Option::None;

                // respect hashbang
                // and put comments below it
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
                                "{}... all target {}\n",
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
            Some(metafile) => {
                return metafile.content.clone();
            }
        }
    }

    fn applysection(&mut self, section: Section) -> bool {
        if let Some(_) = &self.metafile {
            eprintln!(
                "{}",
                "cannot apply individual section to file managed by metafile"
                    .red()
                    .bold()
            );
            return false;
        }
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

// create file with directory creation and
// parsing of the home tilde
// MAYBETODO: support environment variables
// return false if file already exists
fn create_file(path: &str) -> bool {
    let realtargetname = expand_tilde(path);

    let checkpath = Path::new(&realtargetname);
    if !checkpath.is_file() {
        let bufpath = checkpath.to_path_buf();
        match bufpath.parent() {
            Some(parent) => {
                std::fs::create_dir_all(parent.to_str().unwrap()).unwrap();
            }
            None => {}
        }
        File::create(&realtargetname).unwrap();
        return true;
    } else {
        return false;
    }
}

// expand tilde in path into the home folder
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

// detect comment syntax for file based on filename, extension and hashbang
fn get_comment_sign(filename: &str, firstline: &str) -> String {
    let fpath = Path::new(filename);

    let mut file_name_commentsigns: HashMap<&str, &str> = HashMap::new();
    file_name_commentsigns.insert("dunstrc", "#");
    file_name_commentsigns.insert("jgmenurc", "#");
    file_name_commentsigns.insert("zshrc", "#");
    file_name_commentsigns.insert("bashrc", "#");
    file_name_commentsigns.insert("Xresources", "!");
    file_name_commentsigns.insert("vimrc", "\"");

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
    file_type_commentsigns.insert("rc", "#");
    file_type_commentsigns.insert("ini", ";");
    file_type_commentsigns.insert("xresources", "!");

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

// return specialfile from specific part of the program arguments
fn get_special_file(
    matches: &ArgMatches,
    name: &str,
) -> Option<Result<Specialfile, std::io::Error>> {
    let filename = matches.value_of(name).unwrap();
    if Path::new(filename).is_file() {
        let retfile = Specialfile::new(filename);
        return Some(retfile);
    }
    None
}

fn main() -> Result<(), std::io::Error> {
    // argument definition for specifying imosid file
    let inputarg = Arg::new("input")
        .multiple_occurrences(true)
        .short('i')
        .long("input")
        .takes_value(true)
        .required(false)
        .about("add file to source list");

    let metafilearg = Arg::new("metafile")
        .required(false)
        .short('m')
        .long("metafile")
        .takes_value(false)
        .about("put imosid metadata into a separate file instead of using comments in the file");

    // parse program arguments using clap
    let matches = App::new("imosid")
        .version(crate_version!())
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
                .setting(AppSettings::ColoredHelp).arg(&metafilearg)
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
            ).arg(Arg::new("recursive")
                  .about("recurively apply all files in the current directly")
                  .takes_value(false)
                  .required(false)
                  .short('r'))
        )
        .setting(AppSettings::ColoredHelp).setting(AppSettings::ArgRequiredElseHelp)
        .get_matches();

    if matches.is_present("compile") {
        if let Some(ref matches) = matches.subcommand_matches("compile") {
            let filename = matches.value_of("file").unwrap();
            if matches.is_present("metafile") {
                let mut newmetafile = Metafile::from(PathBuf::from(filename));
                //TODO reduce multiple hash updates
                newmetafile.compile();
                newmetafile.write_to_file();
            } else {
                if Path::new(filename).is_file() {
                    let mut testfile = Specialfile::new(filename)?;
                    testfile.compile();
                    testfile.write_to_file();
                }
            }
        }
    }

    // show imosid information about file
    if matches.is_present("info") {
        if let Some(ref matches) = matches.subcommand_matches("info") {
            let filename = matches.value_of("file").unwrap();
            if Path::new(filename).is_file() {
                let infofile = Specialfile::new(filename)?;

                match &infofile.metafile {
                    None => {
                        let commentsign = &infofile.commentsign;
                        println!("comment syntax: {}", commentsign);
                        for i in infofile.sections {
                            if !i.name.is_some() {
                                continue;
                            }
                            let mut outstr: String;
                            outstr =
                                format!("{}-{}: {} | ", i.startline, i.endline, i.name.unwrap());
                            if i.modified {
                                outstr.push_str(&format!("{}", "modified".red().bold()));
                            } else {
                                outstr.push_str(&format!("{}", "ok".green().bold()));
                            }
                            match i.source {
                                Some(source) => {
                                    outstr.push_str(" | source ");
                                    outstr.push_str(&source);
                                }
                                None => {}
                            }
                            println!("{}", &outstr);
                        }
                    }
                    Some(metafile) => {
                        println!("metafile hash: {}", &metafile.hash);
                        println!(
                            "{}",
                            if metafile.modified {
                                "modified".red()
                            } else {
                                "unmodified".green()
                            }
                        )
                    }
                }

                match infofile.targetfile {
                    Some(target) => {
                        println!("target: {}", &target);
                    }
                    None => {}
                }
            } else {
                println!("file {} not found", filename);
            }
        }
    }

    if matches.is_present("apply") {
        if let Some(ref matches) = matches.subcommand_matches("apply") {
            if matches.is_present("recursive") {
                if !Path::new(&expand_tilde(matches.value_of("file").unwrap())).is_dir() {
                    eprintln!("cannot apply file as recursive");
                    return Ok(());
                }
                for i in std::fs::read_dir(&matches.value_of("file").unwrap()).unwrap() {
                    let tmpsourcepath = String::from(&i.unwrap().path().display().to_string());
                    if Path::new(&tmpsourcepath).is_dir() {
                        continue;
                    }
                    let tmpsource = match Specialfile::new(&tmpsourcepath) {
                        Ok(file) => file,
                        Err(_) => continue,
                    };
                    if tmpsource.targetfile.is_some() {
                        // todo: combine multiple sources applying to one file into one write

                        let targetpath = String::from(&tmpsource.targetfile.clone().unwrap());
                        println!("applying file {} to {}", &tmpsource.filename, &targetpath);
                        if create_file(&targetpath) {
                            // create new file
                            Specialfile::create_file(tmpsource);
                        } else {
                            let mut targetfile = match Specialfile::new(&expand_tilde(&targetpath))
                            {
                                Ok(file) => file,
                                Err(_) => continue,
                            };
                            if targetfile.applyfile(&tmpsource) {
                                targetfile.write_to_file();
                            }
                        }
                    } else {
                        println!("file {} has no specified target", &tmpsource.filename);
                        return Ok(());
                    }
                }
                return Ok(());
            }

            let sourcefile = get_special_file(&matches, "file");
            if !sourcefile.is_some() {
                eprintln!("error: cannot open file");
                return Ok(());
            }
            let sourcefile = sourcefile.expect("could not open source file")?;
            match &sourcefile.targetfile {
                None => {
                    println!("No target comment found in {}", &sourcefile.filename);
                    return Ok(());
                }
                Some(targetname) => {
                    let realtargetname = expand_tilde(targetname);
                    if create_file(targetname) {
                        println!("created new file");
                        Specialfile::create_file(sourcefile);
                    } else {
                        let mut targetfile = Specialfile::new(&realtargetname)?;
                        if targetfile.applyfile(&sourcefile) {
                            targetfile.write_to_file();
                        } else {
                            println!("failed to apply file");
                        }
                    }
                }
            }
        }
    }

    if matches.is_present("update") {
        if let Some(ref matches) = matches.subcommand_matches("update") {
            //TODO multiple input files

            let mut modified = false;

            let mut targetfile = get_special_file(matches, "target").unwrap()?;

            if matches.is_present("input") {
                let inputfile = get_special_file(matches, "input").unwrap()?;
                modified = targetfile.applyfile(&inputfile);
            } else {
                match &mut targetfile.metafile {
                    None => {
                        if matches.value_of("target").unwrap() == matches.value_of("input").unwrap()
                        {
                            return Ok(());
                        }
                        // cache specialfiles to avoid multiple fs calls
                        let mut applymap: HashMap<&String, Specialfile> = HashMap::new();
                        let mut applyvec = Vec::new();

                        for i in &targetfile.sections {
                            if let Some(source) = &i.source {
                                if !applymap.contains_key(source) {
                                    match Specialfile::new(source) {
                                        Ok(applyfile) => {
                                            applymap.insert(source, applyfile);
                                        }
                                        Err(_) => {
                                            println!("failed to apply section {}", source);
                                            continue;
                                        }
                                    }
                                }
                                if applymap.contains_key(source) {
                                    applyvec.push(
                                        applymap
                                            .get(source)
                                            .unwrap()
                                            .clone()
                                            .get_section(source)
                                            .unwrap(),
                                    );
                                }
                            }
                        }

                        for i in applyvec.iter() {
                            targetfile.applysection(i.clone());
                        }
                    }
                    Some(metafile) => {
                        if !metafile.modified {
                            if let Some(sourcefile) = &metafile.sourcefile {
                                match Specialfile::new(sourcefile) {
                                    Ok(file) => {
                                        modified = targetfile.applyfile(&file);
                                    }
                                    Err(_) => {
                                        println!("failed to apply metafile source {}", sourcefile);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if matches.is_present("print") {
                println!("{}", targetfile.output());
            } else {
                if modified {
                    targetfile.write_to_file();
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
                let testfile = Specialfile::new(filename)?;

                match &testfile.metafile {
                    None => match matches.values_of("section") {
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
                    },
                    Some(metafile) => {
                        println!("{}", metafile.content);
                    }
                }
            } else {
                eprintln!("file {} not found", filename);
            }
        }
    }
    Ok(())
}
