mod app;

use clap::ArgMatches;
use colored::Colorize;
use dirs::home_dir;
use regex::Regex;
use semver::Version;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, read_to_string, File, OpenOptions};
use std::io::{self, prelude::*, BufRead, ErrorKind};
use std::ops::Deref;
use std::os::unix::prelude::PermissionsExt;
use std::path::{Path, PathBuf};
use toml::Value;
use walkdir::WalkDir;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CommentType {
    SectionBegin,
    SectionEnd,
    SourceInfo,
    TargetInfo,
    HashInfo,
    PermissionInfo,
}

// Use of a mod or pub mod is not actually necessary.
pub mod built_info {
   // The file has been placed there by the build script.
   include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

/// A comment that gets interpreted by imosid
#[derive(Clone)]
pub struct Specialcomment {
    line: u32,
    content: String,
    section: String,
    ctype: CommentType,
    argument: Option<String>,
}

/// A line in a file with its line number and content stored
pub struct ContentLine {
    linenumber: u32,
    content: String,
}

impl Specialcomment {
    fn new(line: &str, commentsymbol: &str, linenumber: u32) -> Option<Specialcomment> {
        if !line.starts_with(commentsymbol) {
            return Option::None;
        }

        // construct regex that matches valid comments
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
                    "permissions" => {
                        if sectionname != "all" {
                            return Option::None;
                        }
                        match &cargument {
                            None => {
                                return Option::None;
                            }
                            Some(arg) => match arg.parse::<u32>() {
                                Err(_) => {
                                    return Option::None;
                                }
                                Ok(_) => {
                                    tmptype = CommentType::PermissionInfo;
                                }
                            },
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

/// A section of a file marked by beginning/end comments
/// Or alternatively no comments for an anonymous section
/// these can be independently updated or broken
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

    /// set target hash to current hash
    /// marking the section as unmodified
    /// return false if nothing has changed
    fn compile(&mut self) -> bool {
        match &self.targethash {
            Some(hash) => {
                if hash.to_string() == self.hash {
                    return false;
                }
            }
            None => {
                if self.is_anonymous() {
                    return false;
                } else {
                    return true;
                }
            }
        }
        self.targethash = Option::Some(self.hash.clone());
        true
    }

    /// anonymous sections are sections without marker comments
    /// e.g. parts not tracked by imosid
    fn is_anonymous(&self) -> bool {
        match &self.name {
            Some(_) => false,
            None => true,
        }
    }

    /// generate section hash
    /// and detect section status
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
    //maybe make this a trait?
    fn push_str(&mut self, line: &str) {
        self.content.push_str(line);
        self.content.push('\n');
    }

    /// return entire section with formatted marker comments and content
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

// a file containing metadata about an imosid file for file types which do not support comments
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
    permissions: Option<u32>,
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
                    permissions: Option::None,
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

                if let Some(Value::Integer(permissions)) = value.get("permissions") {
                    //TODO check if permissions smaller than 777
                    retfile.permissions = Some(*permissions as u32);
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

    fn write_permissions(&self) {
        let mut parentpath = self.path.clone();
        parentpath.pop();
        parentpath.push(&self.parentfile);
        if let Some(permissions) = &self.permissions {
            let mut perms = fs::metadata(&parentpath).unwrap().permissions();
            let permint = u32::from_str_radix(&format!("{}", permissions + 1000000), 8).unwrap();
            perms.set_mode(permint);
            fs::set_permissions(&parentpath, perms).expect("failed to set permissions");
        } else {
            println!("no permissions");
        }
    }

    // create a new metafile for a file
    // TODO maybe return result?
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

        //TODO don't create metafiles for metafiles

        let filename = format!("{}.imosid.toml", parentname);

        path.pop();
        path.push(filename);

        let mut retfile: Metafile;
        //Maybe distinguish between new and from path?
        if path.is_file() {
            retfile = Metafile::new(path.clone(), &filecontent).expect("could not create metafile");
            retfile.update();
            retfile.finalize();
        } else {
            retfile = Metafile {
                targetfile: None,
                sourcefile: None,
                hash: String::from(""),
                parentfile: String::from(&parentname),
                imosidversion: Version::parse(built_info::PKG_VERSION).unwrap(),
                syntaxversion: 0,
                value: Value::Integer(0),
                content: String::from(&filecontent),
                modified: false,
                permissions: Option::None,
                path,
            };

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

    fn compile(&mut self) -> bool {
        let contenthash = self.get_content_hash();
        self.modified = false;
        if self.hash == contenthash {
            false
        } else {
            self.hash = contenthash;
            true
        }
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
    modified: bool,
    permissions: Option<u32>,
}

impl Specialfile {
    fn new(filename: &str) -> Result<Specialfile, std::io::Error> {
        let sourcepath = Path::new(filename)
            .canonicalize()
            .expect("could not canonicalize path")
            .display()
            .to_string();

        let sourcefile = match OpenOptions::new().read(true).write(true).open(filename) {
            Err(e) => {
                if e.kind() == ErrorKind::PermissionDenied {
                    // open file as readonly if writing is not permitted
                    match OpenOptions::new().read(true).write(false).open(filename) {
                        Ok(file) => file,
                        Err(error) => return Err(error),
                    }
                } else {
                    return Err(e);
                }
            }
            Ok(file) => file,
        };

        let metafile;

        let mut commentvector = Vec::new();
        let mut counter = 0;

        let mut sectionvector: Vec<Section> = Vec::new();
        let mut contentvector: Vec<ContentLine> = Vec::new();

        let mut sectionmap: HashMap<String, Vec<Specialcomment>> = HashMap::new();

        let mut targetfile: Option<String> = Option::None;
        let mut permissions = Option::None;
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
                modified: metafile.modified,
                permissions: metafile.permissions.clone(),
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
                                CommentType::PermissionInfo => {
                                    if let Some(arg) = comment.argument {
                                        permissions = match arg.split_at(3).1.parse::<u32>() {
                                            Err(_) => Option::None,
                                            Ok(permnumber) => Option::Some(permnumber),
                                        }
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

            let mut modified = false;
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
                    if i.modified {
                        modified = true;
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
                modified,
                permissions,
            };

            return Ok(retfile);
        }
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

    fn compile(&mut self) -> bool {
        let mut didsomething = false;
        match &mut self.metafile {
            None => {
                for i in 0..self.sections.len() {
                    didsomething = self.sections[i].compile() || didsomething;
                }
            }
            Some(metafile) => {
                didsomething = metafile.compile();
            }
        }
        didsomething
    }

    fn write_to_file(&mut self) {
        let targetname = &expand_tilde(&self.filename);
        let newfile = File::create(targetname);
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

        if let Some(permissions) = self.permissions {
            let mut perms = fs::metadata(targetname).unwrap().permissions();
            let permint = u32::from_str_radix(&format!("{}", permissions + 1000000), 8).unwrap();
            perms.set_mode(permint);
            println!("setting permissions");
            fs::set_permissions(targetname, perms).expect("failed to set permissions");
        }
    }

    fn create_file(source: Specialfile) -> bool {
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
                    modified: source.modified,
                    permissions: source.permissions,
                };
                targetfile.write_to_file();
                return true;
            }
            Some(metafile) => {
                if metafile.modified {
                    println!(
                        "{}",
                        format!("{} modified, skipping", &source.filename).yellow()
                    );
                    return false;
                }
                OpenOptions::new()
                    .write(true)
                    .open(&realtargetpath)
                    .expect(&format!("cannot open file {}", &targetpath))
                    .write_all(metafile.content.as_bytes())
                    .expect(&format!("could not write file {}", &targetpath));
                let mut newmetafile = Metafile::from(PathBuf::from(&realtargetpath));
                newmetafile.sourcefile = Some(source.filename);
                newmetafile.permissions = metafile.permissions;
                newmetafile.write_to_file();
                newmetafile.write_permissions();
                return true;
            }
        }
    }

    fn is_anonymous(&self) -> bool {
        let mut anonymous = true;

        for i in &self.sections {
            if !i.is_anonymous() {
                anonymous = false;
                break;
            }
        }
        anonymous
    }

    // return true if file will be modified
    fn applyfile(&mut self, inputfile: &Specialfile) -> bool {
        match &mut self.metafile {
            None => {
                if self.is_anonymous() {
                    eprintln!(
                        "{} {}",
                        "cannot apply to unmanaged file ".yellow(),
                        self.filename.yellow().bold()
                    );
                    return false;
                }
                if inputfile.metafile.is_some() {
                    eprintln!(
                        "cannot apply metafile to normal imosid file {}",
                        self.filename.bold()
                    );
                    return false;
                }

                if inputfile.is_anonymous() {
                    eprintln!(
                        "{} {}",
                        inputfile.filename.red(),
                        "is unmanaged, cannot apply"
                    );
                    return false;
                }
                //if no sections are updated, don't write anything to the file system
                let mut modified = false;

                // true if input file contains all sections that self has
                let mut allsections = true;

                for i in &self.sections {
                    allsections = false;
                    let selfname = match &i.name {
                        Some(name) => name.clone(),
                        None => {
                            continue;
                        }
                    };
                    for u in &inputfile.sections {
                        if let Some(inputname) = u.name.clone() {
                            if inputname == selfname {
                                allsections = true;
                                break;
                            }
                        } else {
                            continue;
                        }
                    }

                    if !allsections {
                        break;
                    }
                }

                if !self.modified && allsections {
                    // copy entire file contents if all sections are unmodified
                    self.sections = inputfile.sections.clone();
                    self.specialcomments = inputfile.specialcomments.clone();
                    println!(
                        "applied all sections from {} to {}",
                        inputfile.filename.bold(),
                        self.filename.bold()
                    );
                    modified = true;
                } else {
                    let mut applycounter = 0;
                    for i in &inputfile.sections {
                        if self.applysection(i.clone()) {
                            applycounter += 1;
                            modified = true;
                        }
                    }
                    if modified {
                        println!(
                            "applied {} sections from {} to {}",
                            applycounter,
                            self.filename.bold(),
                            inputfile.filename.bold()
                        );
                    } else {
                        println!(
                            "applied no sections from {} to {}",
                            self.filename.bold(),
                            inputfile.filename.bold()
                        );
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
                            if metafile.hash == applymetafile.hash {
                                println!("file {} already up to date", self.filename.bold());
                                return false;
                            }
                            metafile.content = applymetafile.content.clone();
                            metafile.hash = applymetafile.hash.clone();

                            println!(
                                "applied {} to {}",
                                inputfile.filename.bold(),
                                self.filename.bold()
                            );
                            return true;
                        }
                    }
                } else {
                    println!(
                        "{}",
                        format!("target {} modified, skipping", &self.filename.bold()).yellow()
                    );
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
    file_name_commentsigns.insert("xsettingsd", "#");
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

    // parse program arguments using clap
    let matches = app::build_app().get_matches();

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
                    if testfile.compile() {
                        testfile.write_to_file();
                        println!("compiled {}", &filename.bold());
                    } else {
                        println!("{} already compiled", &filename.green());
                    }
                } else {
                    eprintln!("{}", "file does not exist".red().bold());
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

                if let Some(permissions) = infofile.permissions {
                    println!("target file permissions: {}", &permissions);
                }

                if let Some(target) = infofile.targetfile {
                    println!("target: {}", &target);
                }
            } else {
                println!("file {} not found", filename);
            }
        }
    }

    if matches.is_present("apply") {
        if let Some(ref matches) = matches.subcommand_matches("apply") {
            let mut donesomething = false;
            let targetname = expand_tilde(matches.value_of("file").unwrap().clone());
            let filepath = Path::new(&targetname);
            if filepath.is_dir() {
                for entry in WalkDir::new(&matches.value_of("file").unwrap())
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    //TODO do the same as with check
                    let tmpsourcepath = String::from(&entry.path().display().to_string());
                    if Path::new(&tmpsourcepath).is_dir()
                        || tmpsourcepath.ends_with(".imosid.toml")
                        || tmpsourcepath.contains("/.git/")
                    {
                        continue;
                    }
                    let tmpsource = match Specialfile::new(&tmpsourcepath) {
                        Ok(file) => file,
                        Err(_) => continue,
                    };
                    if tmpsource.targetfile.is_some() {
                        // todo: combine multiple sources applying to one file into one write

                        let targetpath = String::from(&tmpsource.targetfile.clone().unwrap());
                        let sourcename = &tmpsource.filename.clone();
                        if create_file(&targetpath) {
                            // create new file
                            if Specialfile::create_file(tmpsource) {
                                println!(
                                    "applied {} to create {} ",
                                    &sourcename.green(),
                                    &targetpath.bold()
                                );
                                donesomething = true;
                            }
                        } else {
                            let mut targetfile = match Specialfile::new(&expand_tilde(&targetpath))
                            {
                                Ok(file) => file,
                                Err(_) => {
                                    println!(
                                        "{}",
                                        format!("failed to parse {}", &targetpath).red()
                                    );
                                    continue;
                                }
                            };
                            if targetfile.applyfile(&tmpsource) {
                                targetfile.write_to_file();
                                donesomething = true;
                            }
                        }
                    } else {
                        println!(
                            "{}",
                            format!("file {} has no target", &tmpsource.filename.bold()).red()
                        );
                        continue;
                    }
                }
                if !donesomething {
                    println!("{}", "nothing to do".bold());
                }
                return Ok(());
            } else if filepath.is_file() {
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
                            println!("created new file {}", targetname.bold());
                            Specialfile::create_file(sourcefile);
                        } else {
                            let mut targetfile = Specialfile::new(&realtargetname)?;
                            if targetfile.applyfile(&sourcefile) {
                                targetfile.write_to_file();
                            }
                        }
                    }
                }
            } else {
                eprintln!("{}", "error: cannot open file".red().bold());
                return Ok(());
            }
        }
    }

    if matches.is_present("check") {
        if let Some(ref matches) = matches.subcommand_matches("check") {
            let targetname = expand_tilde(matches.value_of("directory").unwrap().clone());
            let filepath = Path::new(&targetname);
            if !filepath.is_dir() {
                eprintln!("{}", "only directories can be checked".red());
                return Ok(());
            }

            let mut anymodified = false;

            for direntry in WalkDir::new(&matches.value_of("directory").unwrap())
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let tmpsourcepath = String::from(&direntry.path().display().to_string());

                if tmpsourcepath.ends_with(".imosid.toml") || tmpsourcepath.contains("/.git/") {
                    continue;
                }

                let tmpsource = match Specialfile::new(&tmpsourcepath) {
                    Ok(file) => file,
                    Err(_) => continue,
                };
                if tmpsource.modified {
                    println!("{} {}", tmpsource.filename.red().bold(), "modified".red());
                    anymodified = true;
                }
                let mut fileanonymous = true;
                if !tmpsource.metafile.is_some() {
                    for i in &tmpsource.sections {
                        if !i.is_anonymous() {
                            fileanonymous = false;
                        }
                    }
                    if fileanonymous {
                        println!(
                            "{} {}",
                            tmpsource.filename.yellow().bold(),
                            "is unmanaged".yellow()
                        );
                    }
                }
            }
            if !anymodified {
                println!("{}", "no modified files".green());
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
